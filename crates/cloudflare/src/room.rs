//! One lightandseek room: the Cloudflare port of `crates/server`'s room
//! task and socket pump. The object owns a `game::Room` from its first join
//! until it has had no socket for `LINGER`, so a reload resumes the round.
//! Rooms are ephemeral: nothing goes to storage.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use futures::StreamExt;
use game::{ConnId, Step, To};
use worker::js_sys::{Date, Math, Reflect};
use worker::{
    Delay, DurableObject, Env, Request, Response, Result, State, WebSocket, WebSocketPair,
    WebsocketEvent, durable_object, wasm_bindgen_futures, web_sys,
};

const TICK_MS: f64 = 50.0;
/// How long an empty room keeps running, as in `crates/server`. Longer than
/// the game's reconnect grace, so a reload within the grace finds its avatar.
const LINGER_MS: f64 = 15_000.0;
/// Unsent bytes a socket may hold before the room drops it, the stand-in
/// for `crates/server`'s 64-frame queue. A world welcome is under 3 KB and
/// a snapshot about 50 bytes, so a reading client stays far below this, and
/// a dropped player resumes within the game's grace like any reload.
const BUFFERED_CAP: u32 = 256 * 1024;

pub fn valid(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[durable_object(fetch)]
pub struct Room {
    shared: Rc<Shared>,
}

impl std::fmt::Debug for Room {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Room").finish_non_exhaustive()
    }
}

impl DurableObject for Room {
    fn new(_state: State, _env: Env) -> Self {
        Room {
            shared: Rc::new(Shared {
                live: RefCell::new(None),
                next_conn: Cell::new(0),
                next_gen: Cell::new(0),
            }),
        }
    }

    async fn fetch(&self, _req: Request) -> Result<Response> {
        let pair = WebSocketPair::new()?;
        pair.server.accept()?;
        // Recent compatibility dates deliver binary frames as `Blob`, which
        // Rust cannot read synchronously.
        Reflect::set(
            pair.server.as_ref(),
            &"binaryType".into(),
            &"arraybuffer".into(),
        )?;
        wasm_bindgen_futures::spawn_local(serve(self.shared.clone(), pair.server));
        Response::from_websocket(pair.client)
    }
}

struct Shared {
    live: RefCell<Option<Live>>,
    next_conn: Cell<u64>,
    /// Tells a tick loop whether the room it was started for still exists.
    next_gen: Cell<u64>,
}

struct Live {
    generation: u64,
    game: game::Room,
    sockets: BTreeMap<ConnId, WebSocket>,
    /// `Date.now()` when the last socket went.
    empty_since: Option<f64>,
}

/// Pumps one socket. Its first frame must be a binary hello. A text frame
/// or protocol error closes this socket only.
async fn serve(shared: Rc<Shared>, ws: WebSocket) {
    let conn = ConnId(shared.next_conn.get() + 1);
    shared.next_conn.set(conn.0);
    let Ok(mut events) = ws.events() else { return };
    let hello = match events.next().await {
        Some(Ok(WebsocketEvent::Message(m))) => m.bytes(),
        _ => None,
    };
    if hello.is_some_and(|h| shared.join(conn, &h, &ws)) {
        while let Some(Ok(WebsocketEvent::Message(m))) = events.next().await {
            match m.bytes() {
                Some(bytes) if shared.frame(conn, &bytes) => {}
                _ => break,
            }
        }
        shared.leave(conn);
    }
    let _ = ws.close(Some(1000), None::<&str>);
}

impl Shared {
    fn join(self: &Rc<Self>, conn: ConnId, hello: &[u8], ws: &WebSocket) -> bool {
        let mut slot = self.live.borrow_mut();
        let live = match slot.as_mut() {
            Some(live) => live,
            None => {
                let generation = self.next_gen.get() + 1;
                self.next_gen.set(generation);
                // Runs only once this borrow is gone: spawned futures start
                // on a later microtask.
                wasm_bindgen_futures::spawn_local(tick_loop(self.clone(), generation));
                slot.insert(Live {
                    generation,
                    game: game::Room::new(seed()),
                    sockets: BTreeMap::new(),
                    empty_since: None,
                })
            }
        };
        // A refused hello leaves a fresh room empty, and it retires after
        // its linger like any other.
        let Ok(step) = live.game.join(conn, hello) else {
            return false;
        };
        live.sockets.insert(conn, ws.clone());
        live.empty_since = None;
        live.perform(step);
        true
    }

    /// False once this socket should close: a protocol error, or the room
    /// already let go of it.
    fn frame(&self, conn: ConnId, bytes: &[u8]) -> bool {
        let mut slot = self.live.borrow_mut();
        let Some(live) = slot.as_mut().filter(|l| l.sockets.contains_key(&conn)) else {
            return false;
        };
        match live.game.frame(conn, bytes) {
            Ok(step) => {
                live.perform(step);
                true
            }
            Err(_) => {
                live.leave(conn);
                false
            }
        }
    }

    fn leave(&self, conn: ConnId) {
        if let Some(live) = self.live.borrow_mut().as_mut() {
            live.leave(conn);
        }
    }
}

/// Ticks the room every 50 ms on a fixed schedule, catching up after a late
/// wake like the tokio server's burst interval, and retires it once it has
/// been empty for `LINGER_MS`. Ends as soon as its room is gone.
async fn tick_loop(shared: Rc<Shared>, generation: u64) {
    let mut due = Date::now() + TICK_MS;
    loop {
        let wait = (due - Date::now()).max(0.0);
        Delay::from(Duration::from_millis(wait as u64)).await;
        due += TICK_MS;
        let mut slot = shared.live.borrow_mut();
        let Some(live) = slot.as_mut().filter(|l| l.generation == generation) else {
            return;
        };
        let step = live.game.tick();
        live.perform(step);
        if !live.sockets.is_empty() {
            live.empty_since = None;
            continue;
        }
        let now = Date::now();
        if now - *live.empty_since.get_or_insert(now) >= LINGER_MS {
            *slot = None;
            return;
        }
    }
}

impl Live {
    /// Closing a socket ends its pump, whose `leave` then finds nothing.
    fn leave(&mut self, conn: ConnId) {
        let Some(ws) = self.sockets.remove(&conn) else {
            return;
        };
        let _ = ws.close(Some(1000), None::<&str>);
        let step = self.game.leave(conn);
        self.perform(step);
    }

    /// Never waits on a socket. One that is `BUFFERED_CAP` behind, or whose
    /// send fails, leaves.
    fn perform(&mut self, step: Step) {
        let mut failed = Vec::new();
        for (to, frame) in step.send {
            let bytes = frame.as_bytes();
            let mut send = |(conn, ws): (&ConnId, &WebSocket)| {
                let behind =
                    AsRef::<web_sys::WebSocket>::as_ref(ws).buffered_amount() > BUFFERED_CAP;
                if behind || ws.send_with_bytes(bytes).is_err() {
                    failed.push(*conn);
                }
            };
            match to {
                To::One(c) => self
                    .sockets
                    .get_key_value(&c)
                    .into_iter()
                    .for_each(&mut send),
                To::All => self.sockets.iter().for_each(&mut send),
            }
        }
        // A resume moved the player to a new socket. The old one holds no
        // player now, so the room needs no `leave` for it.
        for conn in step.close {
            if let Some(ws) = self.sockets.remove(&conn) {
                let _ = ws.close(Some(1000), Some("resumed elsewhere"));
            }
        }
        for conn in failed {
            self.leave(conn);
        }
    }
}

/// Not security-relevant: it only varies scenes and spawns between rooms.
fn seed() -> u64 {
    let half = || (Math::random() * 4_294_967_296.0) as u64;
    (half() << 32) | half()
}

#[cfg(test)]
mod tests {
    use super::valid;

    #[test]
    fn room_names_match_the_route_pattern() {
        for ok in ["a", "room-1", "0", &"x".repeat(64)] {
            assert!(valid(ok), "{ok}");
        }
        for bad in ["", "Room", "a_b", "a/b", "..", "é", &"x".repeat(65)] {
            assert!(!valid(bad), "{bad}");
        }
    }
}
