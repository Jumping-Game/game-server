use std::net::SocketAddr;
use std::time::Duration;

use axum::http::{self as axum_http, header};
use axum::response::Response;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use server::config::Config;
use server::errors::ErrorCode;
use server::http::{self, HttpState};
use server::protocol::{self, OutboundMessage, SERVER_PV};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tower::ServiceExt;

async fn spawn_http_server(mut config: Config) -> (HttpState, SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    config.bind_address = addr.to_string();
    config.ws_url = format!("ws://{}/v1/ws", addr);

    let state = HttpState::new(config);
    let router: Router = http::router(state.clone());

    let std_listener = listener.into_std().unwrap();
    std_listener.set_nonblocking(true).unwrap();
    let server = axum::Server::from_tcp(std_listener)
        .unwrap()
        .serve(router.into_make_service());
    let handle = tokio::spawn(async move {
        if let Err(err) = server.await {
            panic!("server error: {err}");
        }
    });

    // Give the server a moment to come online before returning.
    tokio::time::sleep(Duration::from_millis(25)).await;

    (state, addr, handle)
}

fn build_request<B>(req: axum_http::Request<B>) -> axum_http::Request<B> {
    req
}

async fn response_json(response: Response) -> serde_json::Value {
    let status = response.status();
    assert!(status.is_success(), "unexpected status: {status}");
    let bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn rest_create_room_validates_inputs() {
    let config = Config::default();
    let state = HttpState::new(config.clone());
    let app = http::router(state.clone());

    // Invalid region should surface INVALID_STATE as per spec §5.1.
    let bad_region = json!({
        "name": "host",
        "region": "us-west-2",
        "maxPlayers": 4,
        "mode": "endless"
    });
    let response = app
        .clone()
        .oneshot(build_request(
            axum_http::Request::builder()
                .method(axum_http::Method::POST)
                .uri("/v1/rooms")
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(bad_region.to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), axum_http::StatusCode::BAD_REQUEST);
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["code"].as_str(), Some("INVALID_STATE"));

    // Zero maxPlayers should be rejected.
    let zero_capacity = json!({
        "name": "host",
        "region": config.region,
        "maxPlayers": 0,
        "mode": "endless"
    });
    let response = app
        .oneshot(build_request(
            axum_http::Request::builder()
                .method(axum_http::Method::POST)
                .uri("/v1/rooms")
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(zero_capacity.to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), axum_http::StatusCode::BAD_REQUEST);
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["code"].as_str(), Some("INVALID_STATE"));
}

#[tokio::test]
async fn rest_join_room_enforces_capacity_and_name_rules() {
    let config = Config {
        room_capacity: 2,
        ..Config::default()
    };
    let state = HttpState::new(config.clone());
    let app = http::router(state.clone());

    let create_body = json!({
        "name": "host",
        "region": config.region,
        "maxPlayers": 1,
        "mode": "endless"
    });
    let create_response = app
        .clone()
        .oneshot(build_request(
            axum_http::Request::builder()
                .method(axum_http::Method::POST)
                .uri("/v1/rooms")
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(create_body.to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), axum_http::StatusCode::CREATED);
    let body = hyper::body::to_bytes(create_response.into_body())
        .await
        .unwrap();
    let bootstrap: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let room_id = bootstrap["roomId"].as_str().unwrap().to_string();

    // Room should be full because host occupies the only slot.
    let join_uri = format!("/v1/rooms/{}/join", room_id);
    let join_body = json!({ "name": "guest" });
    let response = app
        .clone()
        .oneshot(build_request(
            axum_http::Request::builder()
                .method(axum_http::Method::POST)
                .uri(&join_uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(join_body.to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), axum_http::StatusCode::CONFLICT);
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["code"].as_str(), Some("ROOM_FULL"));

    // Create a room with free capacity to test NAME_TAKEN handling.
    let create_body = json!({
        "name": "host",
        "region": config.region,
        "maxPlayers": 2,
        "mode": "endless"
    });
    let create_response = app
        .clone()
        .oneshot(build_request(
            axum_http::Request::builder()
                .method(axum_http::Method::POST)
                .uri("/v1/rooms")
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(create_body.to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), axum_http::StatusCode::CREATED);
    let body = hyper::body::to_bytes(create_response.into_body())
        .await
        .unwrap();
    let bootstrap: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let join_uri = format!("/v1/rooms/{}/join", bootstrap["roomId"].as_str().unwrap());
    let duplicate = json!({ "name": "host" });
    let response = app
        .oneshot(build_request(
            axum_http::Request::builder()
                .method(axum_http::Method::POST)
                .uri(&join_uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(duplicate.to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), axum_http::StatusCode::CONFLICT);
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["code"].as_str(), Some("NAME_TAKEN"));
}

#[tokio::test]
async fn rest_leave_room_requires_valid_token() {
    let config = Config::default();
    let state = HttpState::new(config.clone());
    let app = http::router(state.clone());

    let create_body = json!({
        "name": "host",
        "region": config.region,
        "maxPlayers": 2,
        "mode": "endless"
    });
    let response = app
        .clone()
        .oneshot(build_request(
            axum_http::Request::builder()
                .method(axum_http::Method::POST)
                .uri("/v1/rooms")
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(create_body.to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let bootstrap: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let room_id = bootstrap["roomId"].as_str().unwrap().to_string();

    // Join as second player to obtain a token.
    let join_uri = format!("/v1/rooms/{}/join", room_id);
    let join_body = json!({ "name": "guest" });
    let response = app
        .clone()
        .oneshot(build_request(
            axum_http::Request::builder()
                .method(axum_http::Method::POST)
                .uri(&join_uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(join_body.to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let guest_bootstrap: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let guest_token = guest_bootstrap["wsToken"].as_str().unwrap().to_string();

    // Attempt to leave with a token from a different room should fail.
    let other_room = state
        .matchmaker
        .create_room(server::matchmaker::CreateRoomRequest {
            name: "other".to_string(),
            region: config.region.clone(),
            max_players: 2,
            mode: "endless".to_string(),
        })
        .await
        .unwrap();
    let leave_uri = format!("/v1/rooms/{}/leave", room_id);
    let response = app
        .clone()
        .oneshot(build_request(
            axum_http::Request::builder()
                .method(axum_http::Method::POST)
                .uri(&leave_uri)
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", other_room.ws_token),
                )
                .body(axum::body::Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), axum_http::StatusCode::UNAUTHORIZED);
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["code"].as_str(), Some("UNAUTHORIZED"));

    // Leaving with the correct token should succeed and return 204.
    let response = app
        .oneshot(build_request(
            axum_http::Request::builder()
                .method(axum_http::Method::POST)
                .uri(&leave_uri)
                .header(header::AUTHORIZATION, format!("Bearer {}", guest_token))
                .body(axum::body::Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), axum_http::StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn rest_status_reports_region_and_version() {
    let config = Config::default();
    let state = HttpState::new(config.clone());
    let app = http::router(state);

    let response = app
        .oneshot(build_request(
            axum_http::Request::builder()
                .method(axum_http::Method::GET)
                .uri("/v1/status")
                .body(axum::body::Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), axum_http::StatusCode::OK);
    let status = response_json(response).await;
    assert_eq!(status["serverPv"].as_u64(), Some(SERVER_PV as u64));
    assert_eq!(status["regions"].as_array().unwrap().len(), 1);
    assert_eq!(
        status["regions"][0]["id"].as_str(),
        Some(config.region.as_str())
    );
    assert_eq!(
        status["regions"][0]["wsUrl"].as_str(),
        Some(config.ws_url.as_str())
    );
}

#[tokio::test]
async fn websocket_join_sequence_matches_spec() {
    let (state, addr, server_handle) = spawn_http_server(Config::default()).await;

    let bootstrap = state
        .matchmaker
        .create_room(server::matchmaker::CreateRoomRequest {
            name: "host".to_string(),
            region: state.config.region.clone(),
            max_players: 2,
            mode: "endless".to_string(),
        })
        .await
        .unwrap();

    let url = url::Url::parse(&format!("ws://{addr}/v1/ws?token={}", bootstrap.ws_token)).unwrap();
    let (mut socket, _response) = tokio_tungstenite::connect_async(url).await.unwrap();

    let join = json!({
        "type": "join",
        "pv": SERVER_PV,
        "seq": 1,
        "ts": 0,
        "payload": {
            "name": "host",
            "clientVersion": "android-0.1.0",
            "device": "Pixel_6_Pro",
            "capabilities": {"tilt": true, "vibrate": true}
        }
    });
    socket.send(Message::Text(join.to_string())).await.unwrap();

    let welcome = expect_outbound(&mut socket, "welcome").await;
    let (player_id, resume_token) = match welcome {
        OutboundMessage::Welcome { payload, .. } => {
            assert_eq!(payload.room_id, bootstrap.room_id);
            assert_eq!(payload.cfg.tps, state.config.tick_rate_hz);
            assert_eq!(payload.cfg.snapshot_rate_hz, state.config.snapshot_rate_hz);
            (payload.player_id.clone(), payload.resume_token.clone())
        }
        other => panic!("expected welcome, got {other:?}"),
    };

    let start = expect_outbound(&mut socket, "start").await;
    if let OutboundMessage::Start { payload, .. } = start {
        assert_eq!(payload.tps, state.config.tick_rate_hz);
    } else {
        panic!("expected start frame");
    }

    let snapshot = expect_outbound(&mut socket, "snapshot").await;
    if let OutboundMessage::Snapshot { payload, .. } = snapshot {
        assert!(payload.full, "first snapshot should be full");
        assert_eq!(payload.stats.dropped_snapshots, 0);
        assert_eq!(payload.players[0].id, player_id.clone());
    } else {
        panic!("expected snapshot frame");
    }

    // Ping/pong per §6 should echo timing information.
    let ping = json!({
        "type": "ping",
        "pv": SERVER_PV,
        "seq": 2,
        "ts": 0,
        "payload": {"t0": 1234567890u64}
    });
    socket.send(Message::Text(ping.to_string())).await.unwrap();
    let pong = expect_outbound(&mut socket, "pong").await;
    if let OutboundMessage::Pong { payload, .. } = pong {
        assert_eq!(payload.t0, 1234567890);
        assert!(payload.t1 >= payload.t0);
    } else {
        panic!("expected pong frame");
    }

    // Valid reconnect should return a snapshot with a resume event.
    let reconnect = json!({
        "type": "reconnect",
        "pv": SERVER_PV,
        "seq": 3,
        "ts": 0,
        "payload": {
            "playerId": player_id,
            "resumeToken": resume_token,
            "lastAckTick": 0
        }
    });
    socket
        .send(Message::Text(reconnect.to_string()))
        .await
        .unwrap();
    let _resume_snapshot = expect_snapshot_with_event(&mut socket, "resume").await;

    // Invalid resume token should raise an UNAUTHORIZED error.
    let bad_reconnect = json!({
        "type": "reconnect",
        "pv": SERVER_PV,
        "seq": 4,
        "ts": 0,
        "payload": {
            "playerId": "p_wrong",
            "resumeToken": "not-valid",
            "lastAckTick": 0
        }
    });
    socket
        .send(Message::Text(bad_reconnect.to_string()))
        .await
        .unwrap();
    let error = expect_outbound(&mut socket, "error").await;
    if let OutboundMessage::Error { payload, .. } = error {
        assert_eq!(payload.code, ErrorCode::Unauthorized);
    } else {
        panic!("expected error frame");
    }

    server_handle.abort();
}

#[tokio::test]
async fn websocket_rejects_out_of_order_messages() {
    let (state, addr, server_handle) = spawn_http_server(Config::default()).await;

    let bootstrap = state
        .matchmaker
        .create_room(server::matchmaker::CreateRoomRequest {
            name: "host".to_string(),
            region: state.config.region.clone(),
            max_players: 2,
            mode: "endless".to_string(),
        })
        .await
        .unwrap();

    let join_resp = state
        .matchmaker
        .join_room(
            &bootstrap.room_id,
            server::matchmaker::JoinRoomRequest {
                name: "guest".to_string(),
            },
        )
        .await
        .unwrap();

    let url = url::Url::parse(&format!("ws://{addr}/v1/ws?token={}", join_resp.ws_token)).unwrap();
    let (mut socket, _response) = tokio_tungstenite::connect_async(url).await.unwrap();

    // Sending an input before join should yield INVALID_STATE.
    let input = json!({
        "type": "input",
        "pv": SERVER_PV,
        "seq": 1,
        "ts": 0,
        "payload": {
            "tick": 10,
            "axisX": 0.1,
            "jump": false
        }
    });
    socket.send(Message::Text(input.to_string())).await.unwrap();
    let error = expect_outbound(&mut socket, "error").await;
    if let OutboundMessage::Error { payload, .. } = error {
        assert_eq!(payload.code, ErrorCode::InvalidState);
    } else {
        panic!("expected error frame");
    }

    // Mismatched protocol version should raise BAD_VERSION and close the socket.
    let join = json!({
        "type": "join",
        "pv": SERVER_PV - 1,
        "seq": 2,
        "ts": 0,
        "payload": {
            "name": "guest",
            "clientVersion": "android-0.1.0"
        }
    });
    socket.send(Message::Text(join.to_string())).await.unwrap();
    let error = expect_outbound(&mut socket, "error").await;
    if let OutboundMessage::Error { payload, .. } = error {
        assert_eq!(payload.code, ErrorCode::BadVersion);
    } else {
        panic!("expected error frame");
    }

    // Subsequent messages should not be delivered after BAD_VERSION.
    let maybe_frame = timeout(Duration::from_millis(200), socket.next())
        .await
        .unwrap();
    match maybe_frame {
        None => {}
        Some(Ok(Message::Close(_))) => {}
        Some(Err(_)) => {}
        Some(Ok(other)) => panic!("unexpected frame after BAD_VERSION: {other:?}"),
    }

    server_handle.abort();
}

async fn expect_outbound(
    socket: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    expected_type: &str,
) -> OutboundMessage {
    loop {
        let frame = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("receive timeout")
            .expect("websocket closed");
        match frame {
            Ok(Message::Text(text)) => {
                let outbound: OutboundMessage = serde_json::from_str(&text).unwrap();
                if outbound_name(&outbound) == expected_type {
                    return outbound;
                }
            }
            Ok(Message::Binary(_)) => panic!("unexpected binary frame"),
            Ok(Message::Close(_)) => panic!("socket closed unexpectedly"),
            Ok(Message::Ping(payload)) => {
                socket.send(Message::Pong(payload)).await.unwrap();
            }
            Ok(Message::Pong(_)) => {}
            Ok(Message::Frame(_)) => {}
            Err(err) => panic!("websocket error: {err}"),
        }
    }
}

fn outbound_name(message: &OutboundMessage) -> &'static str {
    match message {
        OutboundMessage::Welcome { .. } => "welcome",
        OutboundMessage::Start { .. } => "start",
        OutboundMessage::Snapshot { .. } => "snapshot",
        OutboundMessage::Pong { .. } => "pong",
        OutboundMessage::Error { .. } => "error",
        OutboundMessage::Finish { .. } => "finish",
        OutboundMessage::PlayerPresence { .. } => "player_presence",
    }
}

async fn expect_snapshot_with_event(
    socket: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    kind: &str,
) -> protocol::SnapshotPayload {
    loop {
        let outbound = expect_outbound(socket, "snapshot").await;
        if let OutboundMessage::Snapshot { payload, .. } = outbound {
            if payload.events.iter().any(|event| event.kind == kind) {
                return payload;
            }
        }
    }
}
