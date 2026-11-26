//! The room server: a tokio + axum shell around `game::Room`.
//!
//! `GET /ws/{room}` upgrades to a WebSocket that speaks binary `game` frames.
//! Everything else is served from the client's `dist` directory.

pub mod room;

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRequestParts, Path, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior, interval_at, sleep, timeout};
use tower_http::services::ServeDir;
use tracing::{info, warn};

use crate::room::{RoomName, Rooms};

/// A peer that stops reading stalls a write once the kernel buffers fill,
/// and the pump only sees the room drop its queue between writes. Without a
/// bound the stalled socket would stay open forever.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// How long an upgraded socket may wait before sending its hello.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a joined socket may go without sending anything, pongs
/// included. The server pings several times per window, and browsers answer
/// pings on their own, so only a dead peer goes this quiet. Writes alone
/// can't detect one: the kernel takes them until its buffer fills.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug)]
pub struct Config {
    pub dist: PathBuf,
    /// How long a room outlives its last socket.
    pub linger: Duration,
    pub hello_timeout: Duration,
    pub idle_timeout: Duration,
}

impl Config {
    pub fn new(dist: PathBuf) -> Config {
        Config {
            dist,
            linger: room::LINGER,
            hello_timeout: HELLO_TIMEOUT,
            idle_timeout: IDLE_TIMEOUT,
        }
    }
}

#[derive(Debug)]
pub struct Server {
    addr: SocketAddr,
    rooms: Rooms,
    stop: oneshot::Sender<()>,
    http: JoinHandle<io::Result<()>>,
}

impl Server {
    pub async fn start(listener: TcpListener, config: Config) -> io::Result<Server> {
        let addr = listener.local_addr()?;
        let rooms = Rooms::new(config.linger);
        let app = Router::new()
            .route("/ws/{room}", get(upgrade))
            .fallback_service(ServeDir::new(&config.dist))
            .with_state(App {
                rooms: rooms.clone(),
                hello_timeout: config.hello_timeout,
                idle_timeout: config.idle_timeout,
            });
        let (stop, stopped) = oneshot::channel::<()>();
        let http = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
        });
        info!(%addr, "listening");
        Ok(Server {
            addr,
            rooms,
            stop,
            http,
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn rooms(&self) -> &Rooms {
        &self.rooms
    }

    /// Closes every socket and stops every room, then stops listening.
    pub async fn shutdown(self) -> io::Result<()> {
        self.rooms.shutdown().await;
        let _ = self.stop.send(());
        self.http.await.map_err(io::Error::other)?
    }
}

/// What every socket handler shares.
#[derive(Clone, Debug)]
struct App {
    rooms: Rooms,
    hello_timeout: Duration,
    idle_timeout: Duration,
}

impl<S: Send + Sync> FromRequestParts<S> for RoomName {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path(raw) = Path::<String>::from_request_parts(parts, state)
            .await
            .map_err(IntoResponse::into_response)?;
        RoomName::parse(&raw).ok_or_else(|| {
            warn!(room = raw, "rejected room name");
            (StatusCode::BAD_REQUEST, "room must match ^[a-z0-9-]{1,64}$").into_response()
        })
    }
}

async fn upgrade(room: RoomName, State(app): State<App>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| serve_socket(socket, room, app))
}

/// Pumps one socket. Its first binary frame is the hello. Any text frame or
/// protocol error closes this socket only, as does missing `hello_timeout`
/// or `idle_timeout`.
async fn serve_socket(socket: WebSocket, room: RoomName, app: App) {
    let (mut sink, mut stream) = socket.split();
    let hello = timeout(app.hello_timeout, async {
        loop {
            match stream.next().await {
                Some(Ok(Message::Binary(b))) => break Some(b),
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                _ => break None,
            }
        }
    })
    .await;
    let hello = match hello {
        Ok(Some(b)) => b,
        Ok(None) => return,
        Err(_) => {
            warn!(room = room.as_str(), "no hello in time, closing socket");
            let _ = timeout(WRITE_TIMEOUT, sink.send(Message::Close(None))).await;
            return;
        }
    };
    let Some((member, mut inbox)) = app.rooms.join(&room, hello).await else {
        let _ = timeout(WRITE_TIMEOUT, sink.send(Message::Close(None))).await;
        return;
    };
    let idle = app.idle_timeout;
    // `interval_at` panics on a zero period.
    let every = (idle / 3).max(Duration::from_millis(1));
    let mut ping = interval_at(Instant::now() + every, every);
    ping.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let silence = sleep(idle);
    tokio::pin!(silence);
    loop {
        tokio::select! {
            msg = stream.next() => {
                silence.as_mut().reset(Instant::now() + idle);
                match msg {
                    Some(Ok(Message::Binary(b))) => {
                        if !member.frame(b) {
                            break;
                        }
                    }
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                    Some(Ok(Message::Text(_))) => {
                        warn!(room = room.as_str(), conn = member.conn.0, "text frame, closing socket");
                        break;
                    }
                    Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                }
            }
            () = &mut silence => {
                warn!(room = room.as_str(), conn = member.conn.0, "socket went silent, closing");
                break;
            }
            _ = ping.tick() => {
                let sent = timeout(WRITE_TIMEOUT, sink.send(Message::Ping(Bytes::new()))).await;
                if !matches!(sent, Ok(Ok(()))) {
                    break;
                }
            }
            out = inbox.recv() => match out {
                Some(b) => {
                    let sent = timeout(WRITE_TIMEOUT, sink.send(Message::Binary(b))).await;
                    if !matches!(sent, Ok(Ok(()))) {
                        break;
                    }
                }
                None => break,
            },
        }
    }
    member.leave();
    let _ = timeout(WRITE_TIMEOUT, sink.send(Message::Close(None))).await;
}
