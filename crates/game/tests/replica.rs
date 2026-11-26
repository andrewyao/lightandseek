use std::collections::VecDeque;

use game::{
    ConnId, Event, LobbyConfig, Phase, PlayerId, Replica, Role, Room, SceneKind, Step, To, Vec2,
};

const ME: ConnId = ConnId(1);
const TICK_MS: f64 = 50.0;

/// One client against one room over a link with `latency` ticks each way.
/// Each tick: deliver due uplink frames, tick the room, deliver due downlink
/// frames, then the client produces its input.
struct Link {
    room: Room,
    client: Option<Replica>,
    latency: u32,
    now: u32,
    up: VecDeque<(u32, Vec<u8>)>,
    down: VecDeque<(u32, Vec<u8>)>,
    events: Vec<Event>,
}

impl Link {
    fn new(seed: u64, latency: u32) -> Link {
        Link {
            room: Room::new(seed),
            client: None,
            latency,
            now: 0,
            up: VecDeque::new(),
            down: VecDeque::new(),
            events: Vec::new(),
        }
    }

    fn downlink(&mut self, step: Step) {
        for (to, frame) in step.send {
            assert!(matches!(to, To::All | To::One(ME)));
            self.down
                .push_back((self.now + self.latency, frame.as_bytes().to_vec()));
        }
    }

    fn send(&mut self, bytes: Vec<u8>) {
        self.up.push_back((self.now + self.latency, bytes));
    }

    fn connect(&mut self) {
        let step = self
            .room
            .join(ME, Replica::hello("me", None).as_bytes())
            .unwrap();
        self.downlink(step);
    }

    /// Advances one tick: uplink, room tick, downlink.
    fn tick(&mut self) {
        self.now += 1;
        while self.up.front().is_some_and(|(due, _)| *due <= self.now) {
            let (_, bytes) = self.up.pop_front().unwrap();
            let step = self.room.frame(ME, &bytes).unwrap();
            self.downlink(step);
        }
        let step = self.room.tick();
        self.downlink(step);
        let now_ms = f64::from(self.now) * TICK_MS;
        while self.down.front().is_some_and(|(due, _)| *due <= self.now) {
            let (_, bytes) = self.down.pop_front().unwrap();
            match &mut self.client {
                None => self.client = Some(Replica::new(&bytes).unwrap()),
                Some(c) => self.events.extend(c.receive(&bytes, now_ms).unwrap()),
            }
        }
    }

    fn client(&mut self) -> &mut Replica {
        self.client.as_mut().expect("welcomed")
    }

    fn me_on_server(&self, me: PlayerId) -> Vec2 {
        let avatars = self.room.avatars();
        avatars.iter().find(|a| a.id == me).unwrap().body.pos
    }

    fn predicted(&self) -> Vec2 {
        let c = self.client.as_ref().unwrap();
        c.view(f64::from(self.now) * TICK_MS).me.unwrap().body.pos
    }
}

/// A room where the one human is the seeker against one bot, already in
/// the seek phase as the client sees it, so nothing can freeze the human.
fn seeking(latency: u32) -> (Link, PlayerId) {
    for seed in 0..50 {
        let mut link = Link::new(1, latency);
        link.connect();
        for _ in 0..=latency * 2 {
            link.tick();
        }
        let config = LobbyConfig {
            humans: 1,
            bots: 1,
            scene: Some(SceneKind::Kitchen),
            seed: Some(seed),
            hide_secs: 1,
            seek_secs: 120,
        };
        link.send(Replica::configure(config).as_bytes().to_vec());
        link.send(Replica::start().as_bytes().to_vec());
        for _ in 0..60 {
            link.tick();
        }
        let me = link.client().you();
        let now = f64::from(link.now) * TICK_MS;
        let view = link.client().view(now);
        assert!(
            matches!(view.phase, Phase::Seeking { .. }),
            "{:?}",
            view.phase
        );
        if view.seeker == Some(me) {
            assert!(link.events.contains(&Event::RoundStart));
            return (link, me);
        }
    }
    panic!("no seed made the human the seeker");
}

/// The input for client tick `k`: forward with a slowly sweeping yaw, so
/// the path turns and slides along furniture.
fn steer(k: u32) -> (Vec2, f32) {
    (Vec2::new(1.0, 0.3), k as f32 * 0.07)
}

#[test]
fn prediction_matches_the_server_exactly_under_100_ms_latency() {
    let latency = 2;
    let (mut link, me) = seeking(latency);
    let mut predicted_after: Vec<(u32, Vec2)> = Vec::new();
    let mut server_at: Vec<(u32, Vec2)> = Vec::new();
    for k in 0..120 {
        let (mv, yaw) = steer(k);
        let frame = link.client().input(mv, yaw).expect("an avatar to drive");
        link.send(frame.as_bytes().to_vec());
        predicted_after.push((link.now, link.predicted()));
        link.tick();
        server_at.push((link.now, link.me_on_server(me)));
    }
    let start = predicted_after[0].1;
    assert!(
        start.distance(predicted_after[119].1) > 5.0,
        "the avatar travelled"
    );

    // Input sent at client tick t reaches the room `latency` ticks later and
    // is consumed by that tick.
    let mut compared = 0;
    for &(sent, predicted) in &predicted_after {
        let consumed = sent + latency;
        if let Some(&(_, server)) = server_at.iter().find(|(t, _)| *t == consumed) {
            assert_eq!(predicted, server, "input sent at tick {sent}");
            compared += 1;
        }
    }
    assert_eq!(
        compared,
        121 - latency as usize,
        "every consumed input was checked"
    );

    for _ in 0..latency * 2 + 1 {
        let frame = link.client().input(Vec2::ZERO, 0.0).unwrap();
        link.send(frame.as_bytes().to_vec());
        link.tick();
    }
    assert_eq!(link.predicted(), link.me_on_server(me), "settled");
}

#[test]
fn prediction_reconverges_after_a_lost_input() {
    let latency = 2;
    let (mut link, me) = seeking(latency);
    for k in 0..60 {
        let (mv, yaw) = steer(k);
        let frame = link.client().input(mv, yaw).unwrap();
        if k != 20 {
            link.send(frame.as_bytes().to_vec());
        }
        link.tick();
    }
    for _ in 0..10 {
        let frame = link.client().input(Vec2::ZERO, 0.0).unwrap();
        link.send(frame.as_bytes().to_vec());
        link.tick();
    }
    let gap = link.predicted().distance(link.me_on_server(me));
    assert!(gap < 1e-4, "prediction is {gap} from the server after acks");
}

#[test]
fn others_render_interpolated_and_the_hud_counts_hiders() {
    let (mut link, _) = seeking(1);
    let mut seen = Vec::new();
    for _ in 0..20 {
        link.tick();
        let now = f64::from(link.now) * TICK_MS;
        let client = link.client.as_ref().unwrap();
        let view = client.view(now);
        assert_eq!(view.others.len(), 1, "the bot hider");
        assert_eq!(view.others[0].role, Role::Hider);
        assert_eq!(view.hiders_left, 1);
        assert!(view.seconds_left > 100.0 && view.seconds_left <= 120.0);
        assert!(view.world.is_some());
        assert!(!view.blind());
        seen.push(view.others[0].body.pos);
        let half = client.view(now + 25.0).others[0].body.pos;
        let next = client.view(now + 50.0).others[0].body.pos;
        let mid = view.others[0].body.pos.lerp(next, 0.5);
        assert!(half.distance(mid) < 1e-3, "half a tick later is halfway");
    }
}
