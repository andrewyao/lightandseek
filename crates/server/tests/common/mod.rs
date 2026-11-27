//! A real server on 127.0.0.1:0 and native `Replica` clients over WebSocket.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use futures_util::{FutureExt, SinkExt, StreamExt};
use game::{Event, Frame, PlayerId, Replica, Vec2, ViewState};
use server::{Config, Server};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub const ROOM: &str = "test-room";

pub async fn start() -> Server {
    start_with(Config::new("no-dist".into())).await
}

/// A server whose empty rooms stop after `linger`.
pub async fn start_lingering(linger: Duration) -> Server {
    start_with(Config {
        linger,
        ..Config::new("no-dist".into())
    })
    .await
}

pub async fn start_with(config: Config) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    Server::start(listener, config).await.unwrap()
}

#[derive(Debug)]
pub struct Client {
    pub rep: Replica,
    ws: Ws,
    pub events: Vec<Event>,
    /// Binary frames received after the welcome.
    pub frames: usize,
    /// Inputs made during a `play` step, sent after it.
    outbox: Vec<Frame>,
    epoch: Instant,
}

impl Client {
    pub async fn connect(addr: SocketAddr, name: &str) -> Client {
        Client::connect_as(addr, name, None).await
    }

    /// Connects with `resume`, the id from an earlier socket.
    pub async fn connect_as(addr: SocketAddr, name: &str, resume: Option<PlayerId>) -> Client {
        let mut ws = connect_async(format!("ws://{addr}/ws/{ROOM}"))
            .await
            .unwrap()
            .0;
        ws.send(binary(&Replica::hello(name, resume)))
            .await
            .unwrap();
        let welcome = loop {
            match ws.next().await {
                Some(Ok(Message::Binary(b))) => break b,
                Some(Ok(_)) => continue,
                other => panic!("expected a welcome, got {other:?}"),
            }
        };
        Client {
            rep: Replica::new(&welcome).unwrap(),
            ws,
            events: Vec::new(),
            frames: 0,
            outbox: Vec::new(),
            epoch: Instant::now(),
        }
    }

    pub async fn send(&mut self, frame: &Frame) {
        self.ws.send(binary(frame)).await.unwrap();
    }

    pub fn now_ms(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64() * 1000.0
    }

    pub fn view(&self) -> ViewState<'_> {
        self.rep.view(self.now_ms())
    }

    /// Queues one tick of input. A no-op until this client has an avatar.
    pub fn input(&mut self, mv: Vec2, yaw: f32) {
        if let Some(f) = self.rep.input(mv, yaw, false) {
            self.outbox.push(f);
        }
    }

    /// Applies every frame that has already arrived.
    async fn drain(&mut self) {
        while let Some(msg) = self.ws.next().now_or_never() {
            match msg {
                Some(Ok(Message::Binary(b))) => {
                    self.frames += 1;
                    let now = self.now_ms();
                    if let Some(e) = self.rep.receive(&b, now).unwrap() {
                        self.events.push(e);
                    }
                }
                Some(Ok(_)) => {}
                other => panic!("socket ended: {other:?}"),
            }
        }
        for f in std::mem::take(&mut self.outbox) {
            self.ws.send(binary(&f)).await.unwrap();
        }
    }
}

/// Every 50 ms, like a browser client: apply what arrived, run `step`, send
/// the inputs it queued. Returns once `step` says done. Panics after `secs`.
pub async fn play(
    clients: &mut [&mut Client],
    secs: u64,
    mut step: impl FnMut(&mut [&mut Client]) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut every = tokio::time::interval(Duration::from_millis(50));
    loop {
        every.tick().await;
        assert!(Instant::now() < deadline, "timed out after {secs} s");
        for c in clients.iter_mut() {
            c.drain().await;
        }
        let done = step(clients);
        for c in clients.iter_mut() {
            c.drain().await;
        }
        if done {
            return;
        }
    }
}

/// Polls `done` for up to five seconds.
pub async fn eventually(done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "timed out");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn binary(f: &Frame) -> Message {
    Message::Binary(f.as_bytes().to_vec().into())
}
