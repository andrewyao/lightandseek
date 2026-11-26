//! Rooms: one tokio task per live room, each the only owner of its
//! `game::Room`. Sockets talk to a room through its channel, and the task
//! ticks the room every 50 ms.
//!
//! A room starts on its first join and stops once it has had no socket for
//! its linger, so a player who reloads comes back to the same round. Rooms
//! are ephemeral: nothing is written to disk.

use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use axum::body::Bytes;
use game::{ConnId, Step, To};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};
use tracing::{info, warn};

const TICK: Duration = Duration::from_millis(50);
/// How long an empty room keeps running. Longer than the game's reconnect
/// grace, so a reload within the grace finds its avatar.
pub const LINGER: Duration = Duration::from_secs(15);

/// A room name that passed `^[a-z0-9-]{1,64}$`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RoomName(String);

impl RoomName {
    pub fn parse(s: &str) -> Option<RoomName> {
        let ok = (1..=64).contains(&s.len())
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        ok.then(|| RoomName(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a room tells one socket.
#[derive(Debug)]
pub enum Out {
    Frame(Bytes),
    Close,
}

#[derive(Debug)]
enum Cmd {
    Join {
        conn: ConnId,
        hello: Bytes,
        out: mpsc::UnboundedSender<Out>,
        /// `Err` means the room refused this socket. A dropped reply means the
        /// room stopped first, and the caller retries.
        reply: oneshot::Sender<Result<(), ()>>,
    },
    Frame {
        conn: ConnId,
        bytes: Bytes,
    },
    Leave {
        conn: ConnId,
    },
    Shutdown,
}

/// The live rooms. The lock guards only this map, never a `game::Room`.
#[derive(Clone, Debug)]
pub struct Rooms {
    reg: Arc<Mutex<Registry>>,
    linger: Duration,
}

#[derive(Debug, Default)]
struct Registry {
    live: HashMap<RoomName, Live>,
    next_epoch: u64,
    next_conn: u64,
    closed: bool,
}

#[derive(Debug)]
struct Live {
    epoch: u64,
    tx: mpsc::UnboundedSender<Cmd>,
    task: JoinHandle<()>,
}

/// A socket's handle on its room after a successful join.
#[derive(Debug)]
pub struct Member {
    pub conn: ConnId,
    tx: mpsc::UnboundedSender<Cmd>,
}

impl Member {
    /// False once the room has stopped.
    pub fn frame(&self, bytes: Bytes) -> bool {
        self.tx
            .send(Cmd::Frame {
                conn: self.conn,
                bytes,
            })
            .is_ok()
    }

    pub fn leave(&self) {
        let _ = self.tx.send(Cmd::Leave { conn: self.conn });
    }
}

impl Rooms {
    /// `linger` is how long a room outlives its last socket.
    pub fn new(linger: Duration) -> Rooms {
        Rooms {
            reg: Arc::default(),
            linger,
        }
    }

    /// Hands a socket's first frame to its room, starting the room if needed.
    /// `None` means refused: a bad hello, or shutdown.
    pub async fn join(
        &self,
        room: &RoomName,
        hello: Bytes,
        out: mpsc::UnboundedSender<Out>,
    ) -> Option<Member> {
        let conn = {
            let mut reg = self.lock();
            reg.next_conn += 1;
            ConnId(reg.next_conn)
        };
        loop {
            let (epoch, tx) = self.live(room)?;
            let (reply, answer) = oneshot::channel();
            let join = Cmd::Join {
                conn,
                hello: hello.clone(),
                out: out.clone(),
                reply,
            };
            if tx.send(join).is_err() {
                self.forget(room, epoch);
                continue;
            }
            match answer.await {
                Ok(Ok(())) => return Some(Member { conn, tx }),
                Ok(Err(())) => return None,
                Err(_) => self.forget(room, epoch),
            }
        }
    }

    /// Stops every room, closing its sockets, and waits for its task. Later
    /// joins are refused.
    pub async fn shutdown(&self) {
        let live: Vec<Live> = {
            let mut reg = self.lock();
            reg.closed = true;
            reg.live.drain().map(|(_, l)| l).collect()
        };
        for l in &live {
            let _ = l.tx.send(Cmd::Shutdown);
        }
        for l in live {
            let _ = l.task.await;
        }
    }

    /// Rooms with a running task.
    pub fn live_count(&self) -> usize {
        self.lock().live.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Registry> {
        self.reg.lock().expect("registry lock")
    }

    fn live(&self, room: &RoomName) -> Option<(u64, mpsc::UnboundedSender<Cmd>)> {
        let mut reg = self.lock();
        if reg.closed {
            return None;
        }
        if let Some(l) = reg.live.get(room) {
            return Some((l.epoch, l.tx.clone()));
        }
        reg.next_epoch += 1;
        let epoch = reg.next_epoch;
        let (tx, rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(run(self.clone(), room.clone(), epoch, rx));
        reg.live.insert(
            room.clone(),
            Live {
                epoch,
                tx: tx.clone(),
                task,
            },
        );
        Some((epoch, tx))
    }

    /// Drops the registry entry for `room` if it is still generation `epoch`.
    fn forget(&self, room: &RoomName, epoch: u64) {
        let mut reg = self.lock();
        if reg.live.get(room).is_some_and(|l| l.epoch == epoch) {
            reg.live.remove(room);
        }
    }

    /// Removes this room from the registry unless more commands are queued.
    /// Commands that slip in after this are dropped, and a dropped `Join`
    /// reply makes the joiner start a fresh room.
    fn retire(&self, room: &RoomName, epoch: u64, rx: &mpsc::UnboundedReceiver<Cmd>) -> bool {
        let mut reg = self.lock();
        if !rx.is_empty() {
            return false;
        }
        if reg.live.get(room).is_some_and(|l| l.epoch == epoch) {
            reg.live.remove(room);
        }
        true
    }
}

/// The std hasher's keys come from OS randomness, and the clock separates
/// rooms started in the same process.
fn entropy() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    RandomState::new().hash_one(nanos)
}

struct Room {
    name: RoomName,
    game: game::Room,
    sockets: HashMap<ConnId, mpsc::UnboundedSender<Out>>,
}

async fn run(rooms: Rooms, name: RoomName, epoch: u64, mut rx: mpsc::UnboundedReceiver<Cmd>) {
    let mut room = Room {
        name: name.clone(),
        game: game::Room::new(entropy()),
        sockets: HashMap::new(),
    };
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Burst);
    let mut empty_since: Option<Instant> = None;
    loop {
        tokio::select! {
            cmd = rx.recv() => {
                let Some(cmd) = cmd else { return };
                if let Cmd::Shutdown = cmd {
                    room.close_all();
                    return;
                }
                room.handle(cmd);
            }
            _ = ticker.tick() => {
                let step = room.game.tick();
                room.perform(step);
            }
        }
        if !room.sockets.is_empty() {
            empty_since = None;
        } else if empty_since.get_or_insert_with(Instant::now).elapsed() >= rooms.linger
            && rooms.retire(&name, epoch, &rx)
        {
            info!(room = name.as_str(), "room empty, stopped");
            return;
        }
    }
}

impl Room {
    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Join {
                conn,
                hello,
                out,
                reply,
            } => match self.game.join(conn, &hello) {
                Err(why) => {
                    warn!(
                        room = self.name.as_str(),
                        conn = conn.0,
                        ?why,
                        "rejected hello"
                    );
                    let _ = reply.send(Err(()));
                }
                Ok(step) => {
                    self.sockets.insert(conn, out);
                    let _ = reply.send(Ok(()));
                    info!(room = self.name.as_str(), conn = conn.0, "join");
                    self.perform(step);
                }
            },
            Cmd::Frame { conn, bytes } => {
                if !self.sockets.contains_key(&conn) {
                    return;
                }
                match self.game.frame(conn, &bytes) {
                    Ok(step) => self.perform(step),
                    Err(why) => {
                        warn!(
                            room = self.name.as_str(),
                            conn = conn.0,
                            ?why,
                            "protocol error, closing socket"
                        );
                        self.leave(conn);
                    }
                }
            }
            Cmd::Leave { conn } => self.leave(conn),
            Cmd::Shutdown => {}
        }
    }

    fn leave(&mut self, conn: ConnId) {
        let Some(out) = self.sockets.remove(&conn) else {
            return;
        };
        let _ = out.send(Out::Close);
        info!(room = self.name.as_str(), conn = conn.0, "leave");
        let step = self.game.leave(conn);
        self.perform(step);
    }

    fn perform(&mut self, step: Step) {
        for (to, frame) in step.send {
            let bytes = Bytes::copy_from_slice(frame.as_bytes());
            let send = |out: &mpsc::UnboundedSender<Out>| {
                let _ = out.send(Out::Frame(bytes.clone()));
            };
            match to {
                To::One(c) => self.sockets.get(&c).into_iter().for_each(send),
                To::All => self.sockets.values().for_each(send),
            }
        }
        // A resume moved the player to a new socket. The old one holds no
        // player now, so the room needs no `leave` for it.
        for conn in step.close {
            if let Some(out) = self.sockets.remove(&conn) {
                let _ = out.send(Out::Close);
            }
        }
    }

    fn close_all(&mut self) {
        for out in self.sockets.values() {
            let _ = out.send(Out::Close);
        }
        self.sockets.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::RoomName;

    #[test]
    fn room_names_match_the_route_pattern() {
        for ok in ["a", "room-1", "0", &"x".repeat(64)] {
            assert!(RoomName::parse(ok).is_some(), "{ok}");
        }
        for bad in ["", "Room", "a_b", "a/b", "..", "é", &"x".repeat(65)] {
            assert!(RoomName::parse(bad).is_none(), "{bad}");
        }
    }
}
