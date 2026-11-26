//! The sync shell: one WebSocket per attempt, binary `game` frames, and a
//! fresh `Replica` per socket.
//!
//! The first frame on each socket is the room's Welcome, which builds the
//! replica. Every Hello after the first carries the id the room gave this
//! tab, kept in `sessionStorage` so a reload resumes the same avatar while a
//! second tab of the same room joins as a new player.

use std::rc::Rc;

use game::{Event, Frame, PlayerId, Replica, WireError};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{BinaryType, MessageEvent, WebSocket};

/// The room from the URL hash, made to fit the server's `^[a-z0-9-]{1,64}$`.
pub fn room_from_hash(hash: &str) -> String {
    let room: String = hash
        .trim_start_matches('#')
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
        .take(64)
        .collect();
    if room.is_empty() {
        "lobby".into()
    } else {
        room
    }
}

/// `ws(s)://<this page's host><this page's directory>ws/<room>`, so the app
/// works under a path prefix.
pub fn url(room: &str) -> String {
    let loc = web_sys::window().expect("window").location();
    let scheme = match loc.protocol().as_deref() {
        Ok("https:") => "wss",
        _ => "ws",
    };
    let host = loc.host().unwrap_or_else(|_| "localhost:8080".into());
    let path = loc.pathname().unwrap_or_default();
    format!("{scheme}://{host}{}ws/{room}", dir(&path))
}

/// Everything up to and including the last `/`.
fn dir(path: &str) -> &str {
    path.rfind('/').map_or("/", |i| &path[..=i])
}

/// A live socket and the handlers that keep it pumping.
#[allow(dead_code, reason = "the closures must live as long as the socket")]
#[derive(Debug)]
pub struct Socket {
    ws: WebSocket,
    on_open: Closure<dyn FnMut()>,
    on_message: Closure<dyn FnMut(MessageEvent)>,
    on_close: Closure<dyn FnMut()>,
}

impl Socket {
    /// Opens a socket. `frame` gets every binary message; `opened` and
    /// `closed` fire once each.
    pub fn open(
        url: &str,
        opened: Rc<dyn Fn()>,
        frame: Rc<dyn Fn(Vec<u8>)>,
        closed: Rc<dyn Fn()>,
    ) -> Option<Socket> {
        let ws = WebSocket::new(url).ok()?;
        ws.set_binary_type(BinaryType::Arraybuffer);
        let on_open = Closure::<dyn FnMut()>::new(move || opened());
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            if let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                frame(js_sys::Uint8Array::new(&buf).to_vec());
            }
        });
        let on_close = Closure::<dyn FnMut()>::new(move || closed());
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        Some(Socket {
            ws,
            on_open,
            on_message,
            on_close,
        })
    }

    fn send(&self, f: &Frame) {
        let _ = self.ws.send_with_u8_array(f.as_bytes());
    }

    fn close(&self) {
        self.ws.set_onclose(None);
        let _ = self.ws.close();
    }
}

fn session_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.session_storage().ok().flatten()
}

fn resume_key(room: &str) -> String {
    format!("lightseek.resume.{room}")
}

fn load_resume(room: &str) -> Option<PlayerId> {
    let s = session_storage()?.get_item(&resume_key(room)).ok()??;
    s.parse().ok().map(PlayerId)
}

fn save_resume(room: &str, id: PlayerId) {
    if let Some(s) = session_storage() {
        let _ = s.set_item(&resume_key(room), &id.0.to_string());
    }
}

/// What one server frame did.
#[derive(Debug)]
pub enum Got {
    /// A Welcome built a new replica.
    Welcome,
    Event(Option<Event>),
}

#[derive(Debug)]
pub struct Session {
    pub room: String,
    pub name: String,
    /// `Some` once the current socket has been welcomed.
    replica: Option<Replica>,
    socket: Option<Socket>,
    /// Consecutive failed attempts, for backoff.
    attempts: u32,
}

impl Session {
    pub fn new(room: String, name: String) -> Session {
        Session {
            room,
            name,
            replica: None,
            socket: None,
            attempts: 0,
        }
    }

    pub fn replica(&self) -> Option<&Replica> {
        self.replica.as_ref()
    }

    pub fn replica_mut(&mut self) -> Option<&mut Replica> {
        self.replica.as_mut()
    }

    pub fn attach(&mut self, socket: Socket) {
        if let Some(old) = self.socket.take() {
            old.close();
        }
        self.socket = Some(socket);
        self.replica = None;
    }

    pub fn opened(&self) {
        if let Some(s) = &self.socket {
            s.send(&Replica::hello(&self.name, load_resume(&self.room)));
        }
    }

    pub fn receive(&mut self, bytes: &[u8], now_ms: f64) -> Result<Got, WireError> {
        match &mut self.replica {
            None => {
                let r = Replica::new(bytes)?;
                save_resume(&self.room, r.you());
                self.replica = Some(r);
                self.attempts = 0;
                Ok(Got::Welcome)
            }
            Some(r) => r.receive(bytes, now_ms).map(Got::Event),
        }
    }

    /// Sends once welcomed; earlier frames are dropped.
    pub fn send(&self, f: &Frame) {
        if let (Some(_), Some(s)) = (&self.replica, &self.socket) {
            s.send(f);
        }
    }

    /// The socket is gone. Returns how long to wait before the next try.
    pub fn closed(&mut self) -> u32 {
        if let Some(old) = self.socket.take() {
            old.close();
        }
        self.replica = None;
        self.attempts = self.attempts.saturating_add(1);
        (250u32 << self.attempts.min(5)).min(5000)
    }
}
