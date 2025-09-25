use server::{config::Config, http};
use tower::ServiceExt;

#[tokio::test]
async fn ws_requires_token() {
    let app = http::router(http::HttpState::new(Config::default()));
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/v1/ws")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
}
