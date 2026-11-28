//! The server half of the protocol: one `Room` per room name.
//!
//! The lobby rules, membership, rounds, bots, and the fixed-step simulation
//! live here so a whole round runs in a test without a socket. The server
//! crate owns the tokio task and the sockets, calls these methods, and
//! performs the returned `Step`.
//!
//! The room task owns its `Room` exclusively, so it needs no lock.
//!
//! Membership per human: Connected(conn) -> Grace(until) -> Gone. A `Hello`
//! with `resume` inside the grace period reclaims the avatar.

use std::collections::{BTreeMap, VecDeque};

use crate::bots::{Brain, Senses};
use crate::geom::Vec2;
use crate::model::{
    AvatarState, Body, Cause, ConnId, Control, Input, Link, LobbyConfig, Member, Phase, PlayerId,
    Refusal, Role, SceneKind, Seat, Status, Tick,
};
use crate::physics::{self, STEP_UP};
use crate::rng::Rng;
use crate::rules::{
    DANGER_SECS, DRINK_SECS, FOOD_BITE, FOOD_HEIGHT, FOOD_REACH, FOOD_REGROW_SECS, FOOD_SECS,
    GRACE_TICKS, MAX_BOTS, MAX_FOOD, MAX_HUMANS, MAX_NAME, RESULTS_TICKS, Vitals, WATER_SECS, Zone,
    can_catch, mobility, secs, zone_at,
};
use crate::wire::{self, ClientMsg, Frame, ServerMsg, WireError};
use crate::world::World;
use crate::world::nav::NavGrid;

/// Queued inputs beyond this drop the oldest, so a burst cannot build lag.
const MAX_QUEUED: usize = 4;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum To {
    One(ConnId),
    /// Every connection that currently has a player.
    All,
}

/// What the shell must do after one call: send each frame, then close each
/// listed connection (a resume took its player over).
#[derive(Debug, Default)]
pub struct Step {
    pub send: Vec<(To, Frame)>,
    pub close: Vec<ConnId>,
}

impl Step {
    fn push(&mut self, to: To, msg: &ServerMsg) {
        self.send.push((to, wire::frame(msg)));
    }
}

#[derive(Debug)]
enum Presence {
    Connected(ConnId),
    Grace { until: Tick },
}

#[derive(Debug)]
struct Human {
    name: String,
    presence: Presence,
}

#[derive(Debug)]
enum Stage {
    Lobby,
    Round(Box<Round>),
}

#[derive(Debug)]
struct Round {
    world: World,
    nav: NavGrid,
    clock: Clock,
    seeker: PlayerId,
    avatars: BTreeMap<PlayerId, Avatar>,
    /// Ticks the seek phase lasts, for the zone's schedule.
    seek: u32,
    /// Bit `i` is set while `world.food[i]` is ripe.
    food: u64,
}

/// A round's phases. `Phase` adds `Lobby` for the wire.
#[derive(Copy, Clone, Debug)]
enum Clock {
    Hiding { until: Tick },
    Seeking { until: Tick },
    Results { until: Tick, winner: Role },
}

impl Clock {
    fn phase(self) -> Phase {
        match self {
            Clock::Hiding { until } => Phase::Hiding { until },
            Clock::Seeking { until } => Phase::Seeking { until },
            Clock::Results { until, winner } => Phase::Results { until, winner },
        }
    }
}

#[derive(Debug)]
struct Avatar {
    state: AvatarState,
    driver: Driver,
    /// Only a hider's change.
    vitals: Vitals,
}

#[derive(Debug)]
enum Driver {
    Human(Inputs),
    Bot(Box<Brain>),
}

/// One human's input stream. The room consumes at most one per tick and
/// repeats the last when the queue is empty, so sending faster buys nothing.
#[derive(Debug, Default)]
struct Inputs {
    queue: VecDeque<Input>,
    /// The last consumed input, repeated while the queue is empty. Its tick
    /// is the `ack`.
    last: Input,
    /// Highest tick accepted, so duplicates and reorders are dropped.
    received: Tick,
}

impl Inputs {
    fn push(&mut self, input: Input) {
        if input.tick <= self.received {
            return;
        }
        self.received = input.tick;
        if self.queue.len() == MAX_QUEUED {
            self.queue.pop_front();
        }
        self.queue.push_back(input);
    }

    /// A repeat of `last` never jumps: one press is one jump.
    fn next(&mut self) -> Input {
        match self.queue.pop_front() {
            Some(i) => {
                self.last = i;
                i
            }
            None => Input {
                jump: false,
                ..self.last
            },
        }
    }

    /// A fresh client numbers its inputs from 1 again.
    fn restart(&mut self) {
        *self = Inputs {
            last: Input {
                yaw: self.last.yaw,
                ..Input::default()
            },
            ..Inputs::default()
        };
    }
}

#[derive(Debug)]
pub struct Room {
    rng: Rng,
    now: Tick,
    /// Never reused.
    next_id: u32,
    config: LobbyConfig,
    host: Option<PlayerId>,
    humans: BTreeMap<PlayerId, Human>,
    /// Stable bot ids, grown as `config.bots` asks for more.
    bots: Vec<PlayerId>,
    stage: Stage,
}

impl Room {
    /// The shell seeds the room from OS entropy. `LobbyConfig::seed`
    /// overrides it at `Start`.
    pub fn new(seed: u64) -> Room {
        Room {
            rng: Rng::new(seed),
            now: Tick(0),
            next_id: 1,
            config: LobbyConfig::default(),
            host: None,
            humans: BTreeMap::new(),
            bots: Vec::new(),
            stage: Stage::Lobby,
        }
    }

    /// Handles a socket's first frame, which must be `Hello`. A `resume` of a
    /// known player reclaims it under the hello's name, and if that player
    /// still had a socket the step closes it.
    pub fn join(&mut self, conn: ConnId, hello: &[u8]) -> Result<Step, WireError> {
        let ClientMsg::Hello { name, resume } = wire::decode(hello)? else {
            return Err(WireError::Unexpected);
        };
        let mut step = Step::default();
        let resumed = resume.filter(|id| self.humans.contains_key(id));
        let you = match resumed {
            Some(id) => {
                let human = self.humans.get_mut(&id).expect("checked above");
                if let Presence::Connected(old) = human.presence {
                    step.push(To::One(old), &ServerMsg::Replaced);
                    step.close.push(old);
                }
                human.presence = Presence::Connected(conn);
                human.name = clean_name(&name, id);
                if let Some(inputs) = self.inputs_mut(id) {
                    inputs.restart();
                }
                id
            }
            None => {
                let id = self.mint();
                let name = clean_name(&name, id);
                self.humans.insert(
                    id,
                    Human {
                        name,
                        presence: Presence::Connected(conn),
                    },
                );
                self.host.get_or_insert(id);
                id
            }
        };
        let roster = self.roster();
        step.push(
            To::One(conn),
            &ServerMsg::Welcome {
                you,
                host: self.host,
                config: self.config,
                roster: roster.clone(),
            },
        );
        if let Stage::Round(r) = &self.stage {
            step.push(
                To::One(conn),
                &ServerMsg::RoundStart {
                    world: r.world.clone(),
                    seeker: r.seeker,
                    roster,
                },
            );
        }
        self.push_roster(&mut step);
        Ok(step)
    }

    /// Handles any later frame. An `Err` means the client broke the protocol:
    /// the shell closes that socket and calls `leave`.
    pub fn frame(&mut self, conn: ConnId, bytes: &[u8]) -> Result<Step, WireError> {
        let from = self.player_of(conn).ok_or(WireError::Unexpected)?;
        let mut step = Step::default();
        match wire::decode(bytes)? {
            ClientMsg::Hello { .. } => return Err(WireError::Unexpected),
            ClientMsg::Input(input) => {
                if let Some(inputs) = self.inputs_mut(from) {
                    inputs.push(input.clamped());
                }
            }
            ClientMsg::Configure(config) => match self.lobby_gate(from) {
                Err(why) => step.push(To::One(conn), &ServerMsg::Refused(why)),
                Ok(()) if !valid(&config) => {
                    step.push(To::One(conn), &ServerMsg::Refused(Refusal::BadConfig));
                }
                Ok(()) => {
                    self.config = config;
                    self.push_roster(&mut step);
                }
            },
            ClientMsg::Start => match self.lobby_gate(from).and_then(|()| self.startable()) {
                Err(why) => step.push(To::One(conn), &ServerMsg::Refused(why)),
                Ok(()) => {
                    if let Some(seed) = self.config.seed {
                        self.rng = Rng::new(seed);
                    }
                    self.start_round(&mut step);
                }
            },
        }
        Ok(step)
    }

    /// The socket closed. Its player freezes for the grace period. A no-op
    /// for a connection that no longer holds a player.
    pub fn leave(&mut self, conn: ConnId) -> Step {
        let mut step = Step::default();
        let Some(id) = self.player_of(conn) else {
            return step;
        };
        let until = self.now.plus(GRACE_TICKS);
        if let Some(h) = self.humans.get_mut(&id) {
            h.presence = Presence::Grace { until };
        }
        if let Some(inputs) = self.inputs_mut(id) {
            inputs.queue.clear();
            inputs.last.mv = Vec2::ZERO;
        }
        self.push_roster(&mut step);
        step
    }

    /// Advances exactly one 50 ms step.
    pub fn tick(&mut self) -> Step {
        let mut step = Step::default();
        self.now = self.now.plus(1);
        self.expire_grace(&mut step);
        self.advance(&mut step);
        self.push_snapshots(&mut step);
        step
    }

    pub fn now(&self) -> Tick {
        self.now
    }

    pub fn phase(&self) -> Phase {
        match &self.stage {
            Stage::Lobby => Phase::Lobby,
            Stage::Round(r) => r.clock.phase(),
        }
    }

    pub fn host(&self) -> Option<PlayerId> {
        self.host
    }

    pub fn config(&self) -> LobbyConfig {
        self.config
    }

    pub fn world(&self) -> Option<&World> {
        match &self.stage {
            Stage::Lobby => None,
            Stage::Round(r) => Some(&r.world),
        }
    }

    /// Every avatar, unfiltered, in id order.
    pub fn avatars(&self) -> Vec<AvatarState> {
        match &self.stage {
            Stage::Lobby => Vec::new(),
            Stage::Round(r) => r.avatars.values().map(|a| a.state).collect(),
        }
    }

    pub fn roster(&self) -> Vec<Member> {
        let round = match &self.stage {
            Stage::Round(r) => Some(r),
            Stage::Lobby => None,
        };
        let mut lobby_seats = self.config.humans as usize;
        let mut out: Vec<Member> = self
            .humans
            .iter()
            .map(|(&id, h)| {
                let online = matches!(h.presence, Presence::Connected(_));
                let playing = match round {
                    Some(r) => r.avatars.contains_key(&id),
                    None if online && lobby_seats > 0 => {
                        lobby_seats -= 1;
                        true
                    }
                    None => false,
                };
                Member {
                    id,
                    name: h.name.clone(),
                    control: Control::Human,
                    seat: if playing {
                        Seat::Playing
                    } else {
                        Seat::Spectating
                    },
                    link: if online { Link::Online } else { Link::Away },
                }
            })
            .collect();
        if let Some(r) = round {
            for (k, id) in self.bots.iter().enumerate() {
                if r.avatars.contains_key(id) {
                    out.push(Member {
                        id: *id,
                        name: format!("Bot {}", k + 1),
                        control: Control::Bot,
                        seat: Seat::Playing,
                        link: Link::Online,
                    });
                }
            }
        }
        out
    }

    fn mint(&mut self) -> PlayerId {
        let id = PlayerId(self.next_id);
        self.next_id += 1;
        id
    }

    fn player_of(&self, conn: ConnId) -> Option<PlayerId> {
        self.humans
            .iter()
            .find(|(_, h)| matches!(h.presence, Presence::Connected(c) if c == conn))
            .map(|(id, _)| *id)
    }

    fn connected(&self) -> impl Iterator<Item = (PlayerId, ConnId)> + '_ {
        self.humans.iter().filter_map(|(id, h)| match h.presence {
            Presence::Connected(c) => Some((*id, c)),
            Presence::Grace { .. } => None,
        })
    }

    fn inputs_mut(&mut self, id: PlayerId) -> Option<&mut Inputs> {
        let Stage::Round(r) = &mut self.stage else {
            return None;
        };
        match &mut r.avatars.get_mut(&id)?.driver {
            Driver::Human(inputs) => Some(inputs),
            Driver::Bot(_) => None,
        }
    }

    fn lobby_gate(&self, from: PlayerId) -> Result<(), Refusal> {
        if self.host != Some(from) {
            Err(Refusal::NotHost)
        } else if !matches!(self.stage, Stage::Lobby) {
            Err(Refusal::NotInLobby)
        } else {
            Ok(())
        }
    }

    fn startable(&self) -> Result<(), Refusal> {
        let have = self.connected().count().min(u8::MAX as usize) as u8;
        let want = self.config.humans;
        if have < want {
            Err(Refusal::HumansMissing { have, want })
        } else if u32::from(want) + u32::from(self.config.bots) < 2 {
            Err(Refusal::TooFewPlayers)
        } else {
            Ok(())
        }
    }

    /// Seats the first `config.humans` connected humans and `config.bots`
    /// bots on a fresh world. Falls back to the lobby when that is not a game.
    fn start_round(&mut self, step: &mut Step) {
        let humans: Vec<PlayerId> = self
            .connected()
            .map(|(id, _)| id)
            .take(self.config.humans as usize)
            .collect();
        while self.bots.len() < self.config.bots as usize {
            let id = self.mint();
            self.bots.push(id);
        }
        let bots = &self.bots[..self.config.bots as usize];
        if humans.is_empty() || humans.len() + bots.len() < 2 {
            self.stage = Stage::Lobby;
            self.push_roster(step);
            return;
        }
        let kind = self
            .config
            .scene
            .unwrap_or_else(|| SceneKind::ALL[self.rng.below(SceneKind::ALL.len())]);
        let world = World::generate(kind, self.rng.next_u64());
        let nav = NavGrid::new(&world);

        let mut ids: Vec<PlayerId> = humans.iter().chain(bots).copied().collect();
        ids.sort();
        let seeker = ids[self.rng.below(ids.len())];
        let mut spawns = world.spawns.iter().copied();
        let mut avatars = BTreeMap::new();
        for &id in std::iter::once(&seeker).chain(ids.iter().filter(|&&id| id != seeker)) {
            let role = if id == seeker {
                Role::Seeker
            } else {
                Role::Hider
            };
            let pos = spawns.next().expect("a world has a spawn per seat");
            let driver = if self.humans.contains_key(&id) {
                Driver::Human(Inputs::default())
            } else {
                Driver::Bot(Box::new(Brain::new(role)))
            };
            let state = AvatarState {
                id,
                body: Body {
                    pos,
                    ..Body::default()
                },
                role,
                status: Status::Active,
            };
            avatars.insert(
                id,
                Avatar {
                    state,
                    driver,
                    vitals: Vitals::FULL,
                },
            );
        }
        let until = self.now.plus(secs(self.config.hide_secs.into()));
        let food = all_ripe(world.food.len());
        let sent = world.clone();
        self.stage = Stage::Round(Box::new(Round {
            world,
            nav,
            clock: Clock::Hiding { until },
            seeker,
            avatars,
            seek: secs(self.config.seek_secs.into()),
            food,
        }));
        let msg = ServerMsg::RoundStart {
            world: sent,
            seeker,
            roster: self.roster(),
        };
        step.push(To::All, &msg);
        self.push_roster(step);
    }

    fn expire_grace(&mut self, step: &mut Step) {
        let now = self.now;
        let gone: Vec<PlayerId> = self
            .humans
            .iter()
            .filter(|(_, h)| matches!(h.presence, Presence::Grace { until } if until <= now))
            .map(|(id, _)| *id)
            .collect();
        if gone.is_empty() {
            return;
        }
        for id in &gone {
            self.humans.remove(id);
            if let Stage::Round(r) = &mut self.stage {
                r.avatars.remove(id);
            }
        }
        if self.host.is_some_and(|h| gone.contains(&h)) {
            let next = self
                .connected()
                .map(|(id, _)| id)
                .next()
                .or_else(|| self.humans.keys().next().copied());
            self.host = next;
        }
        self.settle(step);
        self.push_roster(step);
    }

    /// Ends a live round whose seeker is gone or whose hiders are all out.
    fn settle(&mut self, step: &mut Step) {
        let Stage::Round(r) = &mut self.stage else {
            return;
        };
        if let Clock::Results { .. } = r.clock {
            return;
        }
        let winner = if !r.avatars.contains_key(&r.seeker) {
            Role::Hider
        } else if !r
            .avatars
            .values()
            .any(|a| a.state.role == Role::Hider && a.state.status == Status::Active)
        {
            Role::Seeker
        } else {
            return;
        };
        r.finish(self.now, winner, step);
    }

    fn advance(&mut self, step: &mut Step) {
        let now = self.now;
        let Stage::Round(r) = &mut self.stage else {
            return;
        };
        if let Clock::Results { until, .. } = r.clock {
            if now >= until {
                self.start_round(step);
            }
            return;
        }
        r.simulate(now, &mut self.rng);
        r.sustain(now, &mut self.rng, step);
        if let Clock::Seeking { .. } = r.clock {
            let seeker = r.avatars[&r.seeker].state;
            for a in r.avatars.values_mut() {
                let s = &mut a.state;
                if s.role == Role::Hider
                    && s.status == Status::Active
                    && can_catch(seeker.body, s.body, &r.world)
                {
                    s.status = Status::Caught;
                    let msg = ServerMsg::Caught {
                        hider: s.id,
                        by: seeker.id,
                    };
                    step.push(To::All, &msg);
                }
            }
        }
        // A catch on the deadline tick wins for the seeker, so settle first.
        self.settle(step);
        let Stage::Round(r) = &mut self.stage else {
            return;
        };
        match r.clock {
            Clock::Hiding { until } if now >= until => {
                let seek = secs(self.config.seek_secs.into());
                r.clock = Clock::Seeking {
                    until: now.plus(seek),
                };
            }
            Clock::Seeking { until } if now >= until => r.finish(now, Role::Hider, step),
            _ => {}
        }
    }

    fn push_roster(&self, step: &mut Step) {
        let msg = ServerMsg::Roster {
            host: self.host,
            config: self.config,
            roster: self.roster(),
        };
        step.push(To::All, &msg);
    }

    /// One snapshot per connection. While hiding, the seeker's has no hiders.
    fn push_snapshots(&self, step: &mut Step) {
        let phase = self.phase();
        let (zone, food) = match &self.stage {
            Stage::Lobby => (Zone::EVERYWHERE, 0),
            Stage::Round(r) => (r.zone(self.now), r.food),
        };
        for (id, conn) in self.connected() {
            let (avatars, ack, vitals) = match &self.stage {
                Stage::Lobby => (Vec::new(), Tick(0), None),
                Stage::Round(r) => {
                    let mine = r.avatars.get(&id);
                    let blind = matches!(phase, Phase::Hiding { .. })
                        && mine.is_some_and(|a| a.state.role == Role::Seeker);
                    let avatars = r
                        .avatars
                        .values()
                        .filter(|a| !blind || a.state.role == Role::Seeker)
                        .map(|a| a.state)
                        .collect();
                    let ack = match mine.map(|a| &a.driver) {
                        Some(Driver::Human(inputs)) => inputs.last.tick,
                        _ => Tick(0),
                    };
                    let vitals = mine
                        .filter(|a| a.state.role == Role::Hider && a.state.status == Status::Active)
                        .map(|a| a.vitals);
                    (avatars, ack, vitals)
                }
            };
            let msg = ServerMsg::Snapshot {
                tick: self.now,
                phase,
                ack,
                avatars,
                zone,
                food,
                vitals,
            };
            step.push(To::One(conn), &msg);
        }
    }
}

impl Round {
    fn finish(&mut self, now: Tick, winner: Role, step: &mut Step) {
        self.clock = Clock::Results {
            until: now.plus(RESULTS_TICKS),
            winner,
        };
        step.push(To::All, &ServerMsg::RoundEnd { winner });
    }

    fn zone(&self, now: Tick) -> Zone {
        zone_at(&self.world, self.clock.phase(), self.seek, now)
    }

    /// Hunger, thirst, eating, drinking, and the danger zone, for every
    /// living hider in id order, then food regrowth.
    fn sustain(&mut self, now: Tick, rng: &mut Rng, step: &mut Step) {
        let zone = self.zone(now);
        let Round {
            world,
            food,
            avatars,
            ..
        } = self;
        let per_tick = |s: u32| 1.0 / secs(s) as f32;
        for a in avatars.values_mut() {
            let (s, v) = (&mut a.state, &mut a.vitals);
            if s.role != Role::Hider || s.status != Status::Active {
                continue;
            }
            let body = s.body;
            v.food = (v.food - per_tick(FOOD_SECS)).max(0.0);
            v.water = (v.water - per_tick(WATER_SECS)).max(0.0);
            let wet = body.y <= STEP_UP
                && world
                    .springs
                    .iter()
                    .any(|w| w.center.distance(body.pos) < w.radius);
            if wet {
                v.water = (v.water + per_tick(DRINK_SECS)).min(1.0);
            }
            for (i, f) in world.food.iter().enumerate().take(MAX_FOOD) {
                let bit = 1u64 << i;
                if *food & bit != 0
                    && Vec2::new(f.x, f.z).distance(body.pos) < FOOD_REACH
                    && (body.y - f.y).abs() < FOOD_HEIGHT
                {
                    *food &= !bit;
                    v.food = (v.food + FOOD_BITE).min(1.0);
                }
            }
            v.danger = if zone.contains(body) {
                0
            } else {
                v.danger.saturating_add(1)
            };
            let cause = if v.water <= 0.0 {
                Some(Cause::Thirst)
            } else if u32::from(v.danger) > secs(DANGER_SECS) {
                Some(Cause::Zone)
            } else {
                None
            };
            if let Some(cause) = cause {
                s.status = Status::Died(cause);
                step.push(To::All, &ServerMsg::Died { hider: s.id, cause });
            }
        }
        let regrow = per_tick(FOOD_REGROW_SECS);
        for i in 0..world.food.len().min(MAX_FOOD) {
            let bit = 1u64 << i;
            if *food & bit == 0 && rng.chance(regrow) {
                *food |= bit;
            }
        }
    }

    /// Moves every avatar one step. Bots decide from the state at the start
    /// of the tick, so avatar order does not change what they see.
    fn simulate(&mut self, now: Tick, rng: &mut Rng) {
        let before: Vec<AvatarState> = self.avatars.values().map(|a| a.state).collect();
        let zone = self.zone(now);
        for a in self.avatars.values_mut() {
            let input = match &mut a.driver {
                Driver::Human(inputs) => inputs.next(),
                Driver::Bot(brain) => {
                    let senses = Senses {
                        me: a.state,
                        now,
                        phase: self.clock.phase(),
                        avatars: &before,
                        world: &self.world,
                        nav: &self.nav,
                        zone,
                        vitals: (a.state.role == Role::Hider).then_some(a.vitals),
                        food: self.food,
                    };
                    brain.think(&senses, rng).clamped()
                }
            };
            let s = &mut a.state;
            let food = (s.role == Role::Hider).then_some(a.vitals.food);
            s.body = match mobility(self.clock.phase(), s.role, s.status, food) {
                Some(speed) => physics::step(s.body, &input, speed, &self.world),
                None => physics::fall(s.body, &self.world),
            };
        }
    }
}

/// A mask with the first `n` of `MAX_FOOD` bits set.
fn all_ripe(n: usize) -> u64 {
    match n {
        0 => 0,
        n if n >= MAX_FOOD => u64::MAX,
        n => (1u64 << n) - 1,
    }
}

fn valid(c: &LobbyConfig) -> bool {
    (1..=MAX_HUMANS).contains(&c.humans)
        && c.bots <= MAX_BOTS
        && c.hide_secs >= 1
        && c.seek_secs >= 1
}

fn clean_name(raw: &str, id: PlayerId) -> String {
    let name: String = raw
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME)
        .collect();
    if name.is_empty() {
        format!("Player {}", id.0)
    } else {
        name
    }
}

#[cfg(test)]
mod tests;
