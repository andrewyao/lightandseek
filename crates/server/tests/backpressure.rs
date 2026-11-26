mod common;

use axum::body::Bytes;
use common::{Client, play};
use game::{LobbyConfig, Phase, Replica, SceneKind};
use server::room::{QUEUE, RoomName};

#[tokio::test]
async fn a_socket_that_never_reads_is_dropped_while_the_room_plays_on() {
    let server = common::start().await;
    let mut a = Client::connect(server.addr(), "a").await;
    let room = RoomName::parse(common::ROOM).unwrap();
    let hello = Bytes::copy_from_slice(Replica::hello("stalled", None).as_bytes());
    let (_member, mut stalled) = server.rooms().join(&room, hello).await.unwrap();
    let config = LobbyConfig {
        humans: 2,
        bots: 0,
        scene: Some(SceneKind::Park),
        seed: Some(7),
        hide_secs: 60,
        seek_secs: 60,
    };
    a.send(&Replica::configure(config)).await;
    a.send(&Replica::start()).await;

    play(&mut [&mut a], 10, |_| stalled.is_closed()).await;
    let before = a.frames;
    play(&mut [&mut a], 5, |cs| cs[0].frames >= before + 20).await;
    assert!(
        matches!(a.view().phase, Phase::Hiding { .. }),
        "the round runs on"
    );

    let mut queued = 0;
    while stalled.recv().await.is_some() {
        queued += 1;
    }
    assert_eq!(queued, QUEUE, "the room stopped at a full queue");
    server.shutdown().await.unwrap();
}
