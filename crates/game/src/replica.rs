//! The client half of the protocol.
//!
//! One `Replica` per connection. It holds what the room last said, predicts
//! the local avatar, and interpolates everyone else. The shell forwards
//! bytes, calls `input` once per 50 ms client tick, and reads `view`.
//!
//! Prediction: every unacked `Input` stays in `pending`. On each snapshot
//! the local avatar restarts from the server's position and replays
//! `pending` through `physics::step`, the function the room runs, so with
//! steady delivery the prediction equals the server's result exactly.
//!
//! The crate has no clock, so the shell passes `now_ms` (any monotonic
//! milliseconds) to `receive` and `view` for interpolation.

use std::collections::VecDeque;

use crate::geom::Vec2;
use crate::model::{
    AvatarState, Body, Control, Input, LobbyConfig, Member, Phase, PlayerId, Refusal, Role, Status,
    Tick,
};
use crate::physics;
use crate::rules::{TICK_HZ, mobility};
use crate::wire::{self, ClientMsg, Frame, ServerMsg, WireError};
use crate::world::World;

const TICK_MS: f64 = 1000.0 / TICK_HZ as f64;
/// Other avatars render this far behind the newest snapshot.
const INTERP_MS: f64 = 100.0;
const KEPT_SNAPSHOTS: usize = 10;
/// Unacked inputs beyond this drop the oldest. The room keeps far fewer.
const MAX_PENDING: usize = 64;
/// A clock sample this far off the estimate replaces it outright.
const CLOCK_SNAP_MS: f64 = 250.0;

/// Something the HUD may want to toast. Every frame updates the view.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Roster,
    RoundStart,
    Caught {
        hider: PlayerId,
        by: PlayerId,
    },
    RoundEnd {
        winner: Role,
    },
    Refused(Refusal),
    /// A newer socket resumed this player; this one is about to close.
    Replaced,
}

/// One avatar as the engine should draw it.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AvatarView {
    pub id: PlayerId,
    pub body: Body,
    pub role: Role,
    pub status: Status,
    pub control: Control,
}

/// Everything the engine and HUD read for one frame.
#[derive(Debug)]
pub struct ViewState<'a> {
    pub you: PlayerId,
    pub host: Option<PlayerId>,
    pub config: LobbyConfig,
    pub roster: &'a [Member],
    pub phase: Phase,
    /// `None` in the lobby.
    pub world: Option<&'a World>,
    pub seeker: Option<PlayerId>,
    /// Your avatar, predicted. `None` while spectating or in the lobby.
    pub me: Option<AvatarView>,
    /// Everyone else, interpolated. While hiding, a seeker receives none.
    pub others: Vec<AvatarView>,
    pub hiders_left: usize,
    /// Until the current phase's deadline, by the estimated server clock.
    pub seconds_left: f32,
}

impl ViewState<'_> {
    /// The seeker's screen stays black while hiders hide.
    pub fn blind(&self) -> bool {
        matches!(self.phase, Phase::Hiding { .. })
            && self.me.is_some_and(|m| m.role == Role::Seeker)
    }
}

#[derive(Debug)]
struct Snapshot {
    tick: Tick,
    avatars: Vec<AvatarState>,
}

#[derive(Debug)]
struct Round {
    world: World,
    seeker: PlayerId,
}

#[derive(Debug)]
pub struct Replica {
    you: PlayerId,
    host: Option<PlayerId>,
    config: LobbyConfig,
    roster: Vec<Member>,
    phase: Phase,
    round: Option<Round>,
    /// Oldest first.
    snapshots: VecDeque<Snapshot>,
    /// Unacked inputs, oldest first.
    pending: VecDeque<Input>,
    next_input: u32,
    /// The local avatar: server state plus `pending` replayed.
    predicted: Option<AvatarState>,
    /// Estimated `local_ms - server_tick * TICK_MS`.
    clock: Option<f64>,
}

impl Replica {
    /// The first frame on every socket. `resume` is the id from an earlier
    /// `Replica` of this room, to reclaim its avatar after a reload.
    pub fn hello(name: &str, resume: Option<PlayerId>) -> Frame {
        wire::frame(&ClientMsg::Hello {
            name: name.to_owned(),
            resume,
        })
    }

    pub fn configure(config: LobbyConfig) -> Frame {
        wire::frame(&ClientMsg::Configure(config))
    }

    pub fn start() -> Frame {
        wire::frame(&ClientMsg::Start)
    }

    /// Builds a replica from the room's first frame, which is `Welcome`.
    pub fn new(welcome: &[u8]) -> Result<Replica, WireError> {
        let ServerMsg::Welcome {
            you,
            host,
            config,
            roster,
        } = wire::decode(welcome)?
        else {
            return Err(WireError::Unexpected);
        };
        Ok(Replica {
            you,
            host,
            config,
            roster,
            phase: Phase::Lobby,
            round: None,
            snapshots: VecDeque::new(),
            pending: VecDeque::new(),
            next_input: 1,
            predicted: None,
            clock: None,
        })
    }

    pub fn you(&self) -> PlayerId {
        self.you
    }

    /// Applies one frame from the room. Call it from the socket handler, not
    /// the render loop, so a throttled tab still tracks the room.
    pub fn receive(&mut self, bytes: &[u8], now_ms: f64) -> Result<Option<Event>, WireError> {
        Ok(match wire::decode(bytes)? {
            ServerMsg::Welcome { .. } => return Err(WireError::Unexpected),
            ServerMsg::Roster {
                host,
                config,
                roster,
            } => {
                self.host = host;
                self.config = config;
                self.roster = roster;
                Some(Event::Roster)
            }
            ServerMsg::RoundStart {
                world,
                seeker,
                roster,
            } => {
                self.round = Some(Round { world, seeker });
                self.roster = roster;
                self.snapshots.clear();
                self.pending.clear();
                self.predicted = None;
                Some(Event::RoundStart)
            }
            ServerMsg::Snapshot {
                tick,
                phase,
                ack,
                avatars,
            } => {
                self.observe_clock(tick, now_ms);
                self.phase = phase;
                if phase == Phase::Lobby {
                    self.round = None;
                }
                self.reconcile(ack, &avatars);
                self.snapshots.push_back(Snapshot { tick, avatars });
                if self.snapshots.len() > KEPT_SNAPSHOTS {
                    self.snapshots.pop_front();
                }
                None
            }
            ServerMsg::Caught { hider, by } => Some(Event::Caught { hider, by }),
            ServerMsg::RoundEnd { winner } => Some(Event::RoundEnd { winner }),
            ServerMsg::Refused(why) => Some(Event::Refused(why)),
            ServerMsg::Replaced => Some(Event::Replaced),
        })
    }

    /// One client tick of intent: `mv.x` forward, `mv.y` strafe-right, both
    /// clamped to unit length. Moves the prediction and returns the frame to
    /// send, or `None` when you have no avatar.
    pub fn input(&mut self, mv: Vec2, yaw: f32, jump: bool) -> Option<Frame> {
        let me = self.predicted?;
        let world = &self.round.as_ref()?.world;
        let input = Input {
            tick: Tick(self.next_input),
            mv,
            yaw,
            jump,
        }
        .clamped();
        self.next_input += 1;
        let body = match mobility(self.phase, me.role, me.status) {
            Some(speed) => physics::step(me.body, &input, speed, world),
            None => physics::fall(me.body, world),
        };
        self.predicted = Some(AvatarState { body, ..me });
        if self.pending.len() == MAX_PENDING {
            self.pending.pop_front();
        }
        self.pending.push_back(input);
        Some(wire::frame(&ClientMsg::Input(input)))
    }

    fn reconcile(&mut self, ack: Tick, avatars: &[AvatarState]) {
        while self.pending.front().is_some_and(|i| i.tick <= ack) {
            self.pending.pop_front();
        }
        let (Some(server), Some(round)) = (
            avatars.iter().find(|a| a.id == self.you).copied(),
            self.round.as_ref(),
        ) else {
            self.predicted = None;
            return;
        };
        let mut body = server.body;
        let speed = mobility(self.phase, server.role, server.status);
        for input in &self.pending {
            body = match speed {
                Some(speed) => physics::step(body, input, speed, &round.world),
                None => physics::fall(body, &round.world),
            };
        }
        self.predicted = Some(AvatarState { body, ..server });
    }

    fn observe_clock(&mut self, tick: Tick, now_ms: f64) {
        let sample = now_ms - f64::from(tick.0) * TICK_MS;
        self.clock = Some(match self.clock {
            Some(c) if (sample - c).abs() < CLOCK_SNAP_MS => c + (sample - c) * 0.1,
            _ => sample,
        });
    }

    fn server_ticks(&self, now_ms: f64) -> f64 {
        match (self.clock, self.snapshots.back()) {
            (Some(c), _) => (now_ms - c) / TICK_MS,
            (None, Some(s)) => f64::from(s.tick.0),
            (None, None) => 0.0,
        }
    }

    pub fn view(&self, now_ms: f64) -> ViewState<'_> {
        let control = |id: PlayerId| {
            let m = self.roster.iter().find(|m| m.id == id);
            m.map_or(Control::Human, |m| m.control)
        };
        let me = self.predicted.map(|s| AvatarView {
            id: s.id,
            body: s.body,
            role: s.role,
            status: s.status,
            control: Control::Human,
        });
        let at = self.server_ticks(now_ms) - INTERP_MS / TICK_MS;
        let others = match self.snapshots.back() {
            None => Vec::new(),
            Some(newest) => newest
                .avatars
                .iter()
                .filter(|a| a.id != self.you)
                .map(|a| AvatarView {
                    id: a.id,
                    body: self.interpolate(a, at),
                    role: a.role,
                    status: a.status,
                    control: control(a.id),
                })
                .collect(),
        };
        let hiders_left = self.snapshots.back().map_or(0, |s| {
            s.avatars
                .iter()
                .filter(|a| a.role == Role::Hider && a.status == Status::Active)
                .count()
        });
        let deadline = match self.phase {
            Phase::Lobby => None,
            Phase::Hiding { until } | Phase::Seeking { until } | Phase::Results { until, .. } => {
                Some(until)
            }
        };
        let seconds_left = deadline.map_or(0.0, |u| {
            ((f64::from(u.0) - self.server_ticks(now_ms)) / f64::from(TICK_HZ)).max(0.0) as f32
        });
        ViewState {
            you: self.you,
            host: self.host,
            config: self.config,
            roster: &self.roster,
            phase: self.phase,
            world: self.round.as_ref().map(|r| &r.world),
            seeker: self.round.as_ref().map(|r| r.seeker),
            me,
            others,
            hiders_left,
            seconds_left,
        }
    }

    /// `newest`'s body at server time `at`, blended between the two kept
    /// snapshots around it. Clamps at both ends.
    fn interpolate(&self, newest: &AvatarState, at: f64) -> Body {
        let body_at = |s: &Snapshot| s.avatars.iter().find(|a| a.id == newest.id).map(|a| a.body);
        let mut later: Option<(f64, Body)> = None;
        for s in self.snapshots.iter().rev() {
            let Some(body) = body_at(s) else { break };
            let t = f64::from(s.tick.0);
            if t <= at {
                return match later {
                    Some((lt, lb)) if lt > t => lerp(body, lb, ((at - t) / (lt - t)) as f32),
                    _ => body,
                };
            }
            later = Some((t, body));
        }
        later.map_or(newest.body, |(_, b)| b)
    }
}

fn lerp(a: Body, b: Body, t: f32) -> Body {
    let tau = std::f32::consts::TAU;
    let turn = (b.yaw - a.yaw).rem_euclid(tau);
    let turn = if turn > tau / 2.0 { turn - tau } else { turn };
    Body {
        pos: a.pos.lerp(b.pos, t),
        yaw: a.yaw + turn * t,
        y: a.y + (b.y - a.y) * t,
        vy: b.vy,
    }
}
