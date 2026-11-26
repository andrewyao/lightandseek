mod common;

use std::time::Duration;

use common::{Client, play};
use game::geom::{facing, yaw_toward};
use game::{Event, LobbyConfig, Phase, PlayerId, Replica, Role, SceneKind, Vec2};

const FORWARD: Vec2 = Vec2::new(1.0, 0.0);

fn config(humans: u8, bots: u8, hide_secs: u8, seek_secs: u16) -> LobbyConfig {
    LobbyConfig {
        humans,
        bots,
        scene: Some(SceneKind::Park),
        seed: Some(7),
        hide_secs,
        seek_secs,
    }
}

fn got(c: &Client, want: impl Fn(&Event) -> bool) -> bool {
    c.events.iter().any(want)
}

/// Where `viewer` last saw `id`, from the room's snapshots.
fn seen(viewer: &Client, id: PlayerId) -> Option<Vec2> {
    let view = viewer.view();
    view.others.iter().find(|a| a.id == id).map(|a| a.body.pos)
}

async fn host_starts(a: &mut Client, config: LobbyConfig) {
    a.send(&Replica::configure(config)).await;
    a.send(&Replica::start()).await;
}

#[tokio::test]
async fn two_clients_and_a_bot_play_a_round_to_the_end() {
    let server = common::start_lingering(Duration::from_millis(200)).await;
    let mut a = Client::connect(server.addr(), "a").await;
    let mut b = Client::connect(server.addr(), "b").await;
    let (ida, idb) = (a.rep.you(), b.rep.you());
    host_starts(&mut a, config(2, 1, 3, 2)).await;

    play(&mut [&mut a, &mut b], 5, |cs| {
        cs.iter()
            .all(|c| got(c, |e| *e == Event::RoundStart) && c.view().me.is_some())
            && seen(cs[0], idb).is_some()
            && seen(cs[1], ida).is_some()
    })
    .await;
    let (va, vb) = (a.view(), b.view());
    assert!(va.world.is_some());
    assert_eq!(va.world, vb.world, "both clients got the same world");
    assert_eq!(va.seeker, vb.seeker);
    let seeker = va.seeker.unwrap();
    assert!(
        seeker != ida && seeker != idb,
        "seed 7 makes the bot seek, so both humans see each other while hiding"
    );
    assert!(matches!(vb.phase, Phase::Hiding { .. }));
    drop((va, vb));

    let a_before = seen(&b, ida).unwrap();
    let b_before = seen(&a, idb).unwrap();
    let mut ticks = 0;
    play(&mut [&mut a, &mut b], 5, |cs| {
        ticks += 1;
        if ticks <= 15 {
            cs[0].input(FORWARD, 0.0);
        }
        ticks == 20
    })
    .await;
    assert!(matches!(b.view().phase, Phase::Hiding { .. }));
    let moved = seen(&b, ida).unwrap().distance(a_before);
    assert!(moved > 1.0, "a's inputs moved a in b's snapshots: {moved}");
    assert_eq!(
        seen(&a, idb).unwrap(),
        b_before,
        "b sent nothing and stayed put"
    );

    play(&mut [&mut a, &mut b], 10, |cs| {
        cs.iter()
            .all(|c| got(c, |e| matches!(e, Event::RoundEnd { .. })))
    })
    .await;

    assert_eq!(server.rooms().live_count(), 1);
    drop((a, b));
    common::eventually(|| server.rooms().live_count() == 0).await;
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_seeker_that_reaches_and_faces_a_hider_catches_it() {
    let server = common::start().await;
    let mut a = Client::connect(server.addr(), "a").await;
    let mut b = Client::connect(server.addr(), "b").await;
    host_starts(&mut a, config(2, 0, 4, 30)).await;

    play(&mut [&mut a, &mut b], 5, |cs| {
        cs.iter().all(|c| c.view().me.is_some())
    })
    .await;
    let seeker = a.view().seeker.unwrap();
    let (s, h) = if seeker == a.rep.you() {
        (&mut a, &mut b)
    } else {
        (&mut b, &mut a)
    };
    let (sid, hid) = (s.rep.you(), h.rep.you());

    // The hider parks a metre behind the frozen seeker, outside its catch
    // wedge, so only the seeker's own turn can catch it.
    play(&mut [&mut *s, &mut *h], 40, |cs| {
        let (s, h) = (&cs[0], &cs[1]);
        let hiding = matches!(h.view().phase, Phase::Hiding { .. });
        let hmove = {
            let hv = h.view();
            let me = hv.me.unwrap().body.pos;
            let them = hv.others.iter().find(|o| o.id == sid).unwrap().body;
            let to = them.pos - facing(them.yaw) - me;
            (hiding && to.length() > 0.3).then(|| yaw_toward(to))
        };
        let smove = {
            let sv = s.view();
            let me = sv.me.unwrap().body.pos;
            let hider = sv.others.iter().find(|o| o.id == hid).map(|o| o.body.pos);
            hider.map(|p| {
                let to = p - me;
                let mv = if to.length() > 1.5 {
                    FORWARD
                } else {
                    Vec2::ZERO
                };
                (mv, yaw_toward(to))
            })
        };
        if let Some(yaw) = hmove {
            cs[1].input(FORWARD, yaw);
        }
        if let Some((mv, yaw)) = smove {
            cs[0].input(mv, yaw);
        }
        cs.iter()
            .all(|c| got(c, |e| matches!(e, Event::RoundEnd { .. })))
    })
    .await;

    for c in [&a, &b] {
        let caught = Event::Caught {
            hider: hid,
            by: sid,
        };
        assert!(c.events.contains(&caught), "{:?}", c.events);
        assert!(c.events.contains(&Event::RoundEnd {
            winner: Role::Seeker
        }));
    }
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_solo_player_who_reloads_mid_round_gets_the_same_round_back() {
    let server = common::start().await;
    let mut a = Client::connect(server.addr(), "a").await;
    let id = a.rep.you();
    host_starts(&mut a, config(1, 1, 30, 60)).await;
    play(&mut [&mut a], 5, |cs| {
        got(cs[0], |e| *e == Event::RoundStart) && cs[0].view().me.is_some()
    })
    .await;
    let world = a.view().world.cloned();
    drop(a);

    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(server.rooms().live_count(), 1, "the empty room lingers");

    let mut back = Client::connect_as(server.addr(), "a", Some(id)).await;
    assert_eq!(back.rep.you(), id);
    play(&mut [&mut back], 5, |cs| {
        got(cs[0], |e| *e == Event::RoundStart) && cs[0].view().me.is_some()
    })
    .await;
    let view = back.view();
    assert!(
        matches!(view.phase, Phase::Hiding { .. }),
        "{:?}",
        view.phase
    );
    assert_eq!(view.world.cloned(), world, "the same round, not a new one");
    drop(view);
    server.shutdown().await.unwrap();
}
