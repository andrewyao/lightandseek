mod common;

use std::time::Duration;

use futures_util::StreamExt;
use server::Config;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

fn quick() -> Config {
    Config {
        linger: Duration::from_millis(50),
        hello_timeout: Duration::from_millis(200),
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
