use server::{Config, Server};
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Error;

#[tokio::test]
async fn a_bad_room_name_is_a_400_and_starts_no_room() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = Server::start(listener, Config::new("no-dist".into()))
        .await
        .unwrap();
    for bad in ["Room", "a_b", "%2e%2e", &"x".repeat(65)] {
        let url = format!("ws://{}/ws/{bad}", server.addr());
        match connect_async(&url).await {
            Err(Error::Http(resp)) => assert_eq!(resp.status(), 400, "{bad}"),
            other => panic!("{bad}: expected a 400, got {other:?}"),
        }
    }
    assert_eq!(server.rooms().live_count(), 0);
    server.shutdown().await.unwrap();
}
