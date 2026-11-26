mod common;

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use game::Replica;
use server::Config;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

fn quick() -> Config {
    Config {
        linger: Duration::from_millis(50),
        hello_timeout: Duration::from_millis(200),
        idle_timeout: Duration::from_millis(300),
        ..Config::new("no-dist".into())
    }
}

#[tokio::test]
async fn a_socket_that_never_says_hello_is_closed() {
    let server = common::start_with(quick()).await;
    let (mut ws, _) = connect_async(format!("ws://{}/ws/{}", server.addr(), common::ROOM))
        .await
        .unwrap();
    let ended = timeout(Duration::from_secs(2), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "the server kept a silent socket open");
    assert_eq!(server.rooms().live_count(), 0, "no hello starts no room");
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_peer_that_goes_silent_is_dropped_and_its_room_stops() {
    let server = common::start_with(quick()).await;
    // Says hello, then neither reads nor answers pings: a dead peer whose
    // kernel still takes the server's writes.
    let (mut ws, _) = connect_async(format!("ws://{}/ws/{}", server.addr(), common::ROOM))
        .await
        .unwrap();
    ws.send(Message::Binary(
        Replica::hello("ghost", None).as_bytes().to_vec().into(),
    ))
    .await
    .unwrap();
    common::eventually(|| server.rooms().live_count() == 1).await;
    let stopped = timeout(Duration::from_secs(2), async {
        while server.rooms().live_count() != 0 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    assert!(stopped.is_ok(), "the silent socket kept its room alive");
    drop(ws);
    server.shutdown().await.unwrap();
}
