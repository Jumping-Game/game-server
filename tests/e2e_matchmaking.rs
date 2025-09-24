use server::{config::Config, http, protocol::SERVER_PV};
use tower::ServiceExt;

#[tokio::test]
async fn create_and_join_room() {
    let config = Config::default();
    let state = http::HttpState::new(config);
    let app = http::router(state);
    let create_body = serde_json::json!({
        "name": "host",
        "region": "local",
        "maxPlayers": 4,
        "mode": "endless"
    });
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri("/v1/rooms")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(create_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);
    let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let bootstrap: server::matchmaker::CreateRoomResponse =
        serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(bootstrap.seed.as_u64() > 0, true);

    let join_uri = format!("/v1/rooms/{}/join", bootstrap.room_id);
    let join_body = serde_json::json!({ "name": "guest" });
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::POST)
                .uri(join_uri)
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(join_body.to_string()))
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
    assert_eq!(status.regions.len(), 1);
    assert_eq!(status.regions[0].id, "local");
}
