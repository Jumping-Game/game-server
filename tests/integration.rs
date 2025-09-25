use std::{sync::Arc, time::Duration};

use futures::{SinkExt, StreamExt};
use reqwest::StatusCode;
use serde_json::Value;
use server::{config::Config, http, util, ws::WsServer};
use tokio::{net::TcpListener, sync::oneshot};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing_subscriber::{fmt, EnvFilter};

async fn find_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind")
        .local_addr()
        .unwrap()
        .port()
}

async fn spawn_server() -> (Config, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let _ = fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
    let api_port = find_port().await;
    let ws_port = find_port().await;
    let config = Config {
        api_port,
        ws_port,
        api_bind: format!("127.0.0.1:{api_port}"),
        ws_bind: format!("127.0.0.1:{ws_port}"),
        jwt_secret: "test-secret".into(),
        enable_permessage_deflate: false,
        ..Config::default()
    };
    let state = Arc::new(http::HttpState::new(config.clone()));
    let router = http::router(state.clone());

    let http_listener = TcpListener::bind(&config.api_bind).await.unwrap();
    let std_listener = http_listener.into_std().unwrap();
    std_listener.set_nonblocking(true).unwrap();
    let http_server = axum::Server::from_tcp(std_listener)
        .unwrap()
        .serve(router.into_make_service());
    let ws_server = WsServer::new(state.clone()).run();
    let (tx, rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        tokio::select! {
            res = http_server => { res.unwrap(); }
            res = ws_server => { res.unwrap(); }
            _ = rx => {}
        }
    });
    (config, tx, handle)
}

async fn recv_type(
    stream: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    expected: &str,
) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("timeout")
            .expect("message")
            .expect("ws message");
        if let Message::Text(text) = msg {
            let value: Value = serde_json::from_str(&text).unwrap();
            if value["type"] == expected {
                return value;
            }
        }
    }
}

#[tokio::test]
async fn lobby_to_start_flow() {
    let (config, shutdown, handle) = spawn_server().await;
    let client = reqwest::Client::new();
    let base = format!("http://{}", config.api_bind);

    let create = client
        .post(format!("{}/v1/rooms", base))
        .json(&serde_json::json!({"name":"host","region":"local-dev","maxPlayers":4}))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);
    let master_boot = create.json::<Value>().await.unwrap();
    let room_id = master_boot["roomId"].as_str().unwrap().to_string();
    let master_id = master_boot["playerId"].as_str().unwrap().to_string();
    let master_token = master_boot["wsToken"].as_str().unwrap().to_string();

    let join = client
        .post(format!("{}/v1/rooms/{}/join", base, room_id))
        .json(&serde_json::json!({"name":"guest"}))
        .send()
        .await
        .unwrap();
    assert_eq!(join.status(), StatusCode::OK);
    let member_boot = join.json::<Value>().await.unwrap();
    let member_token = member_boot["wsToken"].as_str().unwrap().to_string();
    let member_id = member_boot["playerId"].as_str().unwrap().to_string();

    let master_ws_url = format!("ws://{}/v1/ws?token={}", config.ws_bind, master_token);
    let member_ws_url = format!("ws://{}/v1/ws?token={}", config.ws_bind, member_token);

    let (mut master_ws, _) = connect_async(master_ws_url).await.unwrap();
    let (mut member_ws, _) = connect_async(member_ws_url).await.unwrap();

    let join_payload = |name: &str| {
        serde_json::to_string(&serde_json::json!({
            "type":"join","pv":1,"seq":1,"ts":util::now_ms(),
            "payload":{"name":name}
        }))
        .unwrap()
    };

    master_ws
        .send(Message::Text(join_payload("host")))
        .await
        .unwrap();
    member_ws
        .send(Message::Text(join_payload("guest")))
        .await
        .unwrap();

    let welcome_master = recv_type(&mut master_ws, "welcome").await;
    assert_eq!(
        welcome_master["payload"]["roomId"].as_str().unwrap(),
        room_id
    );
    let _lobby_master = recv_type(&mut master_ws, "lobby_state").await;
    let welcome_member = recv_type(&mut member_ws, "welcome").await;
    assert_eq!(welcome_member["payload"]["roomId"], room_id);
    let _lobby_member = recv_type(&mut member_ws, "lobby_state").await;

    let member_start = client
        .post(format!("{}/v1/rooms/{}/start", base, room_id))
        .json(&serde_json::json!({"playerId": member_id}))
        .send()
        .await
        .unwrap();
    assert_eq!(member_start.status(), StatusCode::FORBIDDEN);

    let master_start = client
        .post(format!("{}/v1/rooms/{}/start", base, room_id))
        .json(&serde_json::json!({
            "playerId": master_id,
            "countdownSec": 0
        }))
        .send()
        .await
        .unwrap();
    assert!(master_start.status().is_success());

    let countdown_master = recv_type(&mut master_ws, "start_countdown").await;
    assert_eq!(countdown_master["payload"]["countdownSec"], 0);
    let _countdown_member = recv_type(&mut member_ws, "start_countdown").await;
    let _start_master = recv_type(&mut master_ws, "start").await;
    let _start_member = recv_type(&mut member_ws, "start").await;

    master_ws
        .send(Message::Text(
            serde_json::to_string(&serde_json::json!({
                "type":"input","pv":1,"seq":2,"ts":util::now_ms(),
                "payload":{"tick":1,"axisX":0.5}
            }))
            .unwrap(),
        ))
        .await
        .unwrap();
    let snapshot = recv_type(&mut master_ws, "snapshot").await;
    assert!(snapshot["payload"]["tick"].as_u64().unwrap() >= 1);

    shutdown.send(()).ok();
    handle.abort();
}
