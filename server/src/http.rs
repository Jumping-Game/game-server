use crate::{
    auth::TokenIssuer,
    config::Config,
    errors::ServerError,
    matchmaker::{Matchmaker, StatusResponse},
    metrics,
    protocol::WsBootstrap,
    ws,
};
use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Clone)]
pub struct HttpState {
    pub config: Config,
    pub token_issuer: TokenIssuer,
    pub matchmaker: Matchmaker,
}

impl HttpState {
    pub fn new(config: Config) -> Self {
        let token_issuer = TokenIssuer::new(config.token_secret.clone());
        let matchmaker = Matchmaker::new(config.clone(), token_issuer.clone());
        Self {
            config,
            token_issuer,
            matchmaker,
        }
    }
}

pub fn router(state: HttpState) -> Router {
    let state = Arc::new(state);
    metrics::init();

    Router::new()
        .route("/v1/rooms", post(create_room))
        .route("/v1/rooms/:room_id/join", post(join_room))
        .route("/v1/rooms/:room_id/leave", post(leave_room))
        .route("/v1/status", get(status))
        .route("/healthz", get(healthz))
        .route("/metrics", get(metrics::metrics_handler))
        .route("/v1/ws", get(ws::upgrade_handler))
        .with_state(state)
}

async fn create_room(
    State(state): State<Arc<HttpState>>,
) -> Result<Json<WsBootstrap>, ServerError> {
    let bootstrap = state.matchmaker.create_room().map_err(|err| {
        ServerError::with_source(
            crate::errors::ErrorCode::Internal,
            "failed to create room",
            err.into(),
        )
    })?;
    Ok(Json(bootstrap))
}

#[derive(Deserialize)]
struct JoinPath {
    room_id: String,
}

async fn join_room(
    Path(path): Path<JoinPath>,
    State(state): State<Arc<HttpState>>,
) -> Result<Json<WsBootstrap>, ServerError> {
    let bootstrap = state.matchmaker.join_room(&path.room_id)?;
    Ok(Json(bootstrap))
}

async fn leave_room(
    Path(path): Path<JoinPath>,
    State(state): State<Arc<HttpState>>,
) -> Result<axum::http::StatusCode, ServerError> {
    state.matchmaker.leave_room(&path.room_id, "placeholder");
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn status(State(state): State<Arc<HttpState>>) -> Result<Json<StatusResponse>, ServerError> {
    Ok(Json(state.matchmaker.status()))
}

async fn healthz() -> &'static str {
    "ok"
}
