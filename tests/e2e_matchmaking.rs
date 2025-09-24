use server::{config::Config, http, protocol::SERVER_PV};
use tower::ServiceExt;

#[tokio::test]
async fn create_and_join_room() {
    let config = Config::default();
    let state = http::HttpState::new(config);
    let app = http::router(state);
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/v1/rooms")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let bootstrap: server::protocol::WsBootstrap = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(bootstrap.seed.as_u64() > 0, true);

    let join_uri = format!("/v1/rooms/{}/join", bootstrap.room_id);
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri(join_uri)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let status_resp = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::GET)
                .uri("/v1/status")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status_resp.status(), axum::http::StatusCode::OK);
    let body_bytes = hyper::body::to_bytes(status_resp.into_body()).await.unwrap();
    let status: server::matchmaker::StatusResponse = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(status.server_pv, SERVER_PV);
    assert!(status.players_active >= 1);
}
