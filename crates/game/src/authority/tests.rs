use std::collections::{BTreeMap, BTreeSet};

use super::*;

/// A room plus the sockets the shell would hold. Decodes what the room
/// actually sent, so filtering is observed on the wire.
struct Net {
    room: Room,
    open: BTreeSet<ConnId>,
    inbox: BTreeMap<ConnId, Vec<ServerMsg>>,
    /// Every frame in send order, for determinism checks.
    log: Vec<(To, Vec<u8>)>,
}

impl Net {
    fn new(seed: u64) -> Net {
        Net {
            room: Room::new(seed),
            open: BTreeSet::new(),
            inbox: BTreeMap::new(),
            log: Vec::new(),
        }
    }

    fn apply(&mut self, step: Step) {
        for (to, frame) in step.send {
            let targets: Vec<ConnId> = match to {
                To::One(c) => vec![c],
                To::All => self.open.iter().copied().collect(),
            };
            for c in targets {
                let msg = wire::decode(frame.as_bytes()).expect("room frames decode");
                self.inbox.entry(c).or_default().push(msg);
            }
            self.log.push((to, frame.0));
        }
        for c in step.close {
            self.open.remove(&c);
        }
    }

    fn join(&mut self, conn: u64, resume: Option<PlayerId>) -> PlayerId {
        let conn = ConnId(conn);
        self.open.insert(conn);
        let hello = wire::encode(&ClientMsg::Hello {
            name: format!("c{}", conn.0),
            resume,
        });
        let step = self.room.join(conn, &hello).expect("hello accepted");
        self.apply(step);
        let inbox = self.inbox.get(&conn).map(Vec::as_slice).unwrap_or_default();
        let you = inbox.iter().rev().find_map(|m| match m {
            ServerMsg::Welcome { you, .. } => Some(*you),
            _ => None,
        });
        you.expect("a welcome")
    }

    fn send(&mut self, conn: u64, msg: &ClientMsg) {
        let step = self
            .room
            .frame(ConnId(conn), &wire::encode(msg))
            .expect("frame accepted");
        self.apply(step);
    }

    fn leave(&mut self, conn: u64) {
        self.open.remove(&ConnId(conn));
        let step = self.room.leave(ConnId(conn));
        self.apply(step);
    }

    fn tick(&mut self) {
        let step = self.room.tick();
        self.apply(step);
    }

    fn ticks(&mut self, n: u32) {
        for _ in 0..n {
            self.tick();
        }
    }

    fn take(&mut self, conn: u64) -> Vec<ServerMsg> {
        self.inbox.remove(&ConnId(conn)).unwrap_or_default()
    }

    fn configure(&mut self, conn: u64, config: LobbyConfig) {
        self.send(conn, &ClientMsg::Configure(config));
    }

    fn seeker(&self) -> PlayerId {
        let a = self.room.avatars();
        a.iter()
            .find(|a| a.role == Role::Seeker)
            .expect("a seeker")
            .id
    }

    fn tick_until(&mut self, limit: u32, done: impl Fn(Phase) -> bool) {
        for _ in 0..limit {
            if done(self.room.phase()) {
                return;
            }
            self.tick();
        }
        panic!("phase never arrived, at {:?}", self.room.phase());
    }
}

fn config(humans: u8, bots: u8) -> LobbyConfig {
    LobbyConfig {
        humans,
        bots,
        scene: Some(SceneKind::Park),
        seed: Some(7),
        hide_secs: 3,
        seek_secs: 30,
    }
}

fn refusals(msgs: &[ServerMsg]) -> Vec<Refusal> {
    msgs.iter()
        .filter_map(|m| match m {
            ServerMsg::Refused(r) => Some(*r),
            _ => None,
        })
        .collect()
}

fn round_starts(msgs: &[ServerMsg]) -> usize {
    msgs.iter()
        .filter(|m| matches!(m, ServerMsg::RoundStart { .. }))
        .count()
}

fn round_ends(msgs: &[ServerMsg]) -> Vec<Role> {
    msgs.iter()
        .filter_map(|m| match m {
            ServerMsg::RoundEnd { winner } => Some(*winner),
            _ => None,
        })
        .collect()
}

fn last_roster(msgs: &[ServerMsg]) -> (Option<PlayerId>, Vec<Member>) {
    msgs.iter()
        .rev()
        .find_map(|m| match m {
            ServerMsg::Roster { host, roster, .. } | ServerMsg::Welcome { host, roster, .. } => {
                Some((*host, roster.clone()))
            }
            _ => None,
        })
        .expect("a roster")
}

fn last_snapshot(msgs: &[ServerMsg]) -> (Phase, Tick, Vec<AvatarState>) {
    msgs.iter()
        .rev()
        .find_map(|m| match m {
            ServerMsg::Snapshot {
                phase,
                ack,
                avatars,
                ..
            } => Some((*phase, *ack, avatars.clone())),
            _ => None,
        })
        .expect("a snapshot")
}

#[test]
fn only_the_host_may_configure_and_start() {
    let mut net = Net::new(1);
    let a = net.join(1, None);
    net.join(2, None);
    assert_eq!(net.room.host(), Some(a));

    net.configure(2, config(2, 1));
    net.send(2, &ClientMsg::Start);
    assert_eq!(refusals(&net.take(2)), [Refusal::NotHost, Refusal::NotHost]);
    assert_eq!(net.room.config(), LobbyConfig::default());
    assert_eq!(net.room.phase(), Phase::Lobby);

    net.configure(1, config(2, 1));
    assert_eq!(net.room.config(), config(2, 1));
    net.send(1, &ClientMsg::Start);
    assert!(refusals(&net.take(1)).is_empty());
    assert_eq!(
        round_starts(&net.take(2)),
        1,
        "the guest sees the round start"
    );
    assert!(matches!(net.room.phase(), Phase::Hiding { .. }));

    net.configure(1, config(2, 2));
    assert_eq!(refusals(&net.take(1)), [Refusal::NotInLobby]);
}

#[test]
fn start_is_refused_under_two_players_or_missing_humans() {
    let mut net = Net::new(1);
    net.join(1, None);
    net.configure(1, config(1, 0));
    net.send(1, &ClientMsg::Start);
    assert_eq!(refusals(&net.take(1)), [Refusal::TooFewPlayers]);

    net.configure(1, config(2, 0));
    net.send(1, &ClientMsg::Start);
    assert_eq!(
        refusals(&net.take(1)),
        [Refusal::HumansMissing { have: 1, want: 2 }]
    );

    net.configure(1, config(0, 3));
    assert_eq!(refusals(&net.take(1)), [Refusal::BadConfig]);
    net.tick();
    assert_eq!(net.room.phase(), Phase::Lobby);
}

/// Runs one human (standing still) against three bots until the first
/// round ends. Returns the net, holding every frame sent, and the winner.
fn headless_round(seed: u64, scene: SceneKind) -> (Net, Role) {
    let mut net = Net::new(99);
    net.join(1, None);
    net.configure(
        1,
        LobbyConfig {
            scene: Some(scene),
            seed: Some(seed),
            // Spawns are spread out, so the bot seeker needs time to search.
            seek_secs: 90,
            ..config(1, 3)
        },
    );
    net.send(1, &ClientMsg::Start);
    net.tick_until(20 * 100, |p| matches!(p, Phase::Results { .. }));
    let Phase::Results { winner, .. } = net.room.phase() else {
        unreachable!()
    };
    (net, winner)
}

#[test]
fn one_human_and_three_bots_finish_a_round_with_a_winner() {
    let mut seeker_wins = 0;
    for (seed, scene) in (0..6).zip(SceneKind::ALL.into_iter().cycle()) {
        let (mut net, winner) = headless_round(seed, scene);
        let msgs = net.take(1);
        assert_eq!(round_ends(&msgs), [winner], "seed {seed}: one RoundEnd");
        let caught = msgs
            .iter()
            .filter(|m| matches!(m, ServerMsg::Caught { .. }))
            .count();
        if winner == Role::Seeker {
            seeker_wins += 1;
            assert_eq!(caught, 3, "seed {seed}: the seeker caught every hider");
        }

        net.ticks(RESULTS_TICKS);
        assert!(
            matches!(net.room.phase(), Phase::Hiding { .. }),
            "seed {seed}: next round"
        );
        assert_eq!(round_starts(&net.take(1)), 1);
    }
    assert!(
        seeker_wins > 0,
        "bots catch people in at least one of the rounds"
    );
}

#[test]
fn the_same_seed_replays_the_same_frames() {
    let (a, _) = headless_round(3, SceneKind::City);
    let (b, _) = headless_round(3, SceneKind::City);
    assert!(a.log == b.log, "frame streams differ");
    let (c, _) = headless_round(4, SceneKind::City);
    assert!(a.log != c.log, "a different seed plays differently");
}

/// Two humans, so the seeker is always human. Long hide phase so only the
/// grace period can end the round.
fn two_humans() -> (Net, PlayerId, u64) {
    let mut net = Net::new(5);
    let a = net.join(1, None);
    net.join(2, None);
    net.configure(
        1,
        LobbyConfig {
            hide_secs: 60,
            ..config(2, 0)
        },
    );
    net.send(1, &ClientMsg::Start);
    let seeker = net.seeker();
    let conn = if seeker == a { 1 } else { 2 };
    (net, seeker, conn)
}

#[test]
fn a_disconnected_seeker_ends_the_round_after_the_grace_period() {
    let (mut net, _, conn) = two_humans();
    let other = 3 - conn;
    net.leave(conn);
    net.ticks(GRACE_TICKS - 1);
    assert!(
        matches!(net.room.phase(), Phase::Hiding { .. }),
        "still inside the grace period"
    );
    assert!(round_ends(&net.take(other)).is_empty());
    net.tick();
    assert!(matches!(
        net.room.phase(),
        Phase::Results {
            winner: Role::Hider,
            ..
        }
    ));
    assert_eq!(round_ends(&net.take(other)), [Role::Hider]);
}

#[test]
fn a_seeker_who_resumes_within_grace_keeps_the_round_going() {
    let (mut net, seeker, conn) = two_humans();
    net.leave(conn);
    net.ticks(GRACE_TICKS / 2);
    assert_eq!(net.join(10, Some(seeker)), seeker);
    let msgs = net.take(10);
    assert_eq!(round_starts(&msgs), 1, "a resumed client gets the world");
    net.ticks(GRACE_TICKS * 2);
    assert!(matches!(net.room.phase(), Phase::Hiding { .. }));
    assert_eq!(net.seeker(), seeker);

    net.send(
        10,
        &ClientMsg::Input(Input {
            tick: Tick(1),
            mv: Vec2::new(1.0, 0.0),
            yaw: 0.0,
        }),
    );
    net.tick();
    assert_eq!(
        last_snapshot(&net.take(10)).1,
        Tick(1),
        "a restarted input counter is accepted"
    );
}

#[test]
fn resuming_over_an_open_socket_closes_it_and_its_late_leave_is_ignored() {
    let mut net = Net::new(1);
    let a = net.join(1, None);
    let step = net.room.join(
        ConnId(2),
        &wire::encode(&ClientMsg::Hello {
            name: "again".into(),
            resume: Some(a),
        }),
    );
    let step = step.unwrap();
    assert_eq!(step.close, [ConnId(1)]);
    net.open.insert(ConnId(2));
    net.apply(step);
    net.leave(1);
    let (host, roster) = last_roster(&net.take(2));
    assert_eq!(host, Some(a));
    assert_eq!(roster[0].link, Link::Online);
    assert_eq!(
        net.room
            .frame(ConnId(1), &wire::encode(&ClientMsg::Start))
            .err(),
        Some(WireError::Unexpected)
    );
    let hello = wire::encode(&ClientMsg::Hello {
        name: String::new(),
        resume: None,
    });
    assert_eq!(
        net.room.frame(ConnId(2), &hello).err(),
        Some(WireError::Unexpected)
    );
}

#[test]
fn a_resumed_hello_renames_the_player() {
    let mut net = Net::new(1);
    let a = net.join(1, None);
    net.leave(1);
    let step = net.room.join(
        ConnId(2),
        &wire::encode(&ClientMsg::Hello {
            name: "  Ada ".into(),
            resume: Some(a),
        }),
    );
    net.open.insert(ConnId(2));
    net.apply(step.unwrap());
    let (_, roster) = last_roster(&net.take(2));
    assert_eq!((roster[0].id, roster[0].name.as_str()), (a, "Ada"));
}

/// One human plus one bot, run into the seek phase with the human sending
/// `per_tick` identical inputs each tick. Returns the human's positions.
fn walk(per_tick: u32) -> Vec<Vec2> {
    let mut net = Net::new(3);
    let me = net.join(1, None);
    net.configure(
        1,
        LobbyConfig {
            hide_secs: 1,
            ..config(1, 1)
        },
    );
    net.send(1, &ClientMsg::Start);
    net.tick_until(100, |p| matches!(p, Phase::Seeking { .. }));
    let world = net.room.world().unwrap();
    let start = net
        .room
        .avatars()
        .iter()
        .find(|a| a.id == me)
        .unwrap()
        .body
        .pos;
    let yaw = (0..16)
        .map(|i| i as f32 * std::f32::consts::TAU / 16.0)
        .find(|&yaw| world.ray_clear(start, start + crate::geom::facing(yaw) * 4.0, 0.01))
        .expect("an open heading");
    let mut next = 1;
    let mut out = Vec::new();
    for _ in 0..40 {
        for _ in 0..per_tick {
            let input = Input {
                tick: Tick(next),
                mv: Vec2::new(1.0, 0.0),
                yaw,
            };
            net.send(1, &ClientMsg::Input(input));
            next += 1;
        }
        net.tick();
        let avatars = net.room.avatars();
        out.push(avatars.iter().find(|a| a.id == me).unwrap().body.pos);
    }
    out
}

#[test]
fn three_inputs_per_tick_move_no_farther_than_one() {
    let one = walk(1);
    let three = walk(3);
    assert_eq!(one, three, "same path tick for tick");
    assert!(one[0].distance(one[39]) > 2.0, "the avatar did move");
}

#[test]
fn oversized_moves_are_clamped_to_unit_speed() {
    let mut net = Net::new(3);
    let me = net.join(1, None);
    net.configure(
        1,
        LobbyConfig {
            hide_secs: 1,
            ..config(1, 1)
        },
    );
    net.send(1, &ClientMsg::Start);
    net.tick_until(100, |p| matches!(p, Phase::Seeking { .. }));
    let pos = |net: &Net| net.room.avatars().iter().find(|a| a.id == me).unwrap().body;
    let before = pos(&net);
    let input = Input {
        tick: Tick(1),
        mv: Vec2::new(30.0, 40.0),
        yaw: 1.0,
    };
    net.send(1, &ClientMsg::Input(input));
    net.tick();
    let moved = pos(&net).pos.distance(before.pos);
    assert!(
        moved <= crate::rules::SEEKER_SPEED * crate::rules::DT + 1e-5,
        "{moved}"
    );
}

#[test]
fn while_hiding_the_seeker_is_sent_no_hider_positions() {
    let mut net = Net::new(2);
    let a = net.join(1, None);
    let b = net.join(2, None);
    let human_seeker = (0..20).find_map(|seed| {
        let config = LobbyConfig {
            hide_secs: 2,
            seed: Some(seed),
            ..config(2, 1)
        };
        let mut net = Net::new(2);
        net.join(1, None);
        net.join(2, None);
        net.configure(1, config);
        net.send(1, &ClientMsg::Start);
        [a, b].contains(&net.seeker()).then_some(config)
    });
    net.configure(1, human_seeker.expect("some seed picks a human seeker"));
    net.send(1, &ClientMsg::Start);
    net.tick();
    let seeker = net.seeker();
    let (sc, hc) = if seeker == a { (1, 2) } else { (2, 1) };

    let (phase, _, seen) = last_snapshot(&net.take(sc));
    assert!(matches!(phase, Phase::Hiding { .. }));
    assert_eq!(seen.iter().map(|s| s.id).collect::<Vec<_>>(), [seeker]);
    let (_, _, hider_sees) = last_snapshot(&net.take(hc));
    assert_eq!(hider_sees.len(), 3, "hiders see everyone");

    net.tick_until(100, |p| matches!(p, Phase::Seeking { .. }));
    net.tick();
    let (_, _, seen) = last_snapshot(&net.take(sc));
    assert_eq!(seen.len(), 3, "the seeker sees everyone once seeking");
}

#[test]
fn the_host_passes_to_the_earliest_connected_human_when_gone() {
    let mut net = Net::new(1);
    let a = net.join(1, None);
    net.join(2, None);
    let c = net.join(3, None);
    net.leave(2);
    net.leave(1);
    net.ticks(GRACE_TICKS - 1);
    assert_eq!(net.room.host(), Some(a), "kept through the grace period");
    net.tick();
    assert_eq!(net.room.host(), Some(c), "the earliest still connected");
    let (host, roster) = last_roster(&net.take(3));
    assert_eq!(host, Some(c));
    assert_eq!(roster.iter().map(|m| m.id).collect::<Vec<_>>(), [c]);
    net.configure(3, config(1, 1));
    assert!(refusals(&net.take(3)).is_empty());
    assert_eq!(net.room.config(), config(1, 1));
}

#[test]
fn late_joiners_spectate_and_take_a_free_seat_next_round() {
    let mut net = Net::new(4);
    net.join(1, None);
    let b = net.join(2, None);
    net.configure(1, config(2, 1));
    net.send(1, &ClientMsg::Start);
    net.tick();

    let c = net.join(3, None);
    let msgs = net.take(3);
    assert_eq!(round_starts(&msgs), 1, "a late joiner gets the world");
    let (_, roster) = last_roster(&msgs);
    let seat = |roster: &[Member], id| roster.iter().find(|m| m.id == id).unwrap().seat;
    assert_eq!(seat(&roster, c), Seat::Spectating);
    net.tick();
    assert_eq!(
        last_snapshot(&net.take(3)).2.len(),
        3,
        "spectators see the round"
    );
    assert!(net.room.avatars().iter().all(|a| a.id != c));

    net.leave(2);
    net.tick_until(20 * 60, |p| matches!(p, Phase::Results { .. }));
    net.tick_until(RESULTS_TICKS + 1, |p| matches!(p, Phase::Hiding { .. }));
    let ids: Vec<PlayerId> = net.room.avatars().iter().map(|a| a.id).collect();
    assert!(ids.contains(&c), "the spectator took the free seat");
    assert!(!ids.contains(&b));
    let (_, roster) = last_roster(&net.take(3));
    assert_eq!(seat(&roster, c), Seat::Playing);
}

#[test]
fn a_full_lobby_seats_the_earliest_and_spectates_the_rest() {
    let mut net = Net::new(1);
    let a = net.join(1, None);
    let b = net.join(2, None);
    let (_, roster) = last_roster(&net.take(2));
    let seats: Vec<(PlayerId, Seat)> = roster.iter().map(|m| (m.id, m.seat)).collect();
    assert_eq!(seats, [(a, Seat::Playing), (b, Seat::Spectating)]);
}

#[test]
fn idle_players_start_the_seek_spread_out_and_out_of_the_seekers_reach() {
    let n = u64::from(MAX_HUMANS);
    for kind in SceneKind::ALL {
        for seed in 0..40 {
            let mut net = Net::new(seed);
            for c in 1..=n {
                net.join(c, None);
            }
            let config = LobbyConfig {
                scene: Some(kind),
                seed: Some(seed),
                ..config(MAX_HUMANS, 0)
            };
            net.configure(1, config);
            net.send(1, &ClientMsg::Start);
            net.tick_until(20 * 10, |p| matches!(p, Phase::Seeking { .. }));
            net.tick();

            let avatars = net.room.avatars();
            assert_eq!(avatars.len(), MAX_HUMANS as usize);
            let seeker = net.seeker();
            let at = |id| avatars.iter().find(|a| a.id == id).unwrap().body.pos;
            for (i, a) in avatars.iter().enumerate() {
                for b in &avatars[i + 1..] {
                    let d = a.body.pos.distance(b.body.pos);
                    assert!(d >= 6.0, "{kind:?} seed {seed}: {:?}-{:?} {d}", a.id, b.id);
                }
                if a.id != seeker {
                    let d = a.body.pos.distance(at(seeker));
                    assert!(d >= 8.0, "{kind:?} seed {seed}: hider {:?} {d}", a.id);
                    assert!(d > crate::rules::CATCH_DIST);
                    assert_eq!(a.status, Status::Active);
                }
            }
            for c in 1..=n {
                let caught = net
                    .take(c)
                    .into_iter()
                    .any(|m| matches!(m, ServerMsg::Caught { .. }));
                assert!(
                    !caught,
                    "{kind:?} seed {seed}: a catch on the first seek tick"
                );
            }
        }
    }
}
