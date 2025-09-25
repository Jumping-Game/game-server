use crate::{
    auth::{AuthService, ResumeStore},
    config::Config,
    errors::{AppError, ErrorCode},
    lobby::{Lobby, Player, Role},
    proto::LobbyState,
    sim::SimHandle,
    util,
};
use axum::{
    extract::{Path, State},
    http::{HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio::time::Instant;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct HttpState {
    pub config: Config,
    pub lobby: Lobby,
    pub auth: AuthService,
    pub resume: ResumeStore,
    create_bucket: Arc<Mutex<VecDeque<Instant>>>,
}

impl HttpState {
    pub fn new(config: Config) -> Self {
        let auth = AuthService::new(config.jwt_secret.clone());
        Self {
            config,
            lobby: Lobby::new(),
            auth,
            resume: ResumeStore::default(),
            create_bucket: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

pub fn router(state: Arc<HttpState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(HeaderValue::from_static("http://localhost:5173"))
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers(Any);

    Router::new()
        .route("/v1/status", get(status))
        .route("/v1/rooms", post(create_room))
        .route("/v1/rooms/:room_id/join", post(join_room))
        .route("/v1/rooms/:room_id/leave", post(leave_room))
        .route("/v1/rooms/:room_id/start", post(start_room))
        .route("/v1/rooms/:room_id/ready", post(set_ready))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(cors)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    regions: Vec<RegionInfo>,
    server_pv: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RegionInfo {
    id: String,
    ping_ms: u32,
    ws_url: String,
}

async fn status(State(state): State<Arc<HttpState>>) -> Json<StatusResponse> {
    let ws_url = format!("ws://localhost:{}/v1/ws", state.config.ws_port);
    Json(StatusResponse {
        regions: vec![RegionInfo {
            id: state.config.region.clone(),
            ping_ms: 5,
            ws_url,
        }],
        server_pv: crate::proto::PROTOCOL_VERSION,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRoomRequest {
    name: String,
    #[serde(default)]
    region: Option<String>,
    #[serde(default = "default_max_players")]
    max_players: u32,
}

const fn default_max_players() -> u32 {
    4
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateRoomResponse {
    room_id: String,
    seed: String,
    region: String,
    ws_url: String,
    ws_token: String,
    role: String,
    state: String,
    max_players: u32,
    player_id: String,
}

async fn create_room(
    State(state): State<Arc<HttpState>>,
    Json(req): Json<CreateRoomRequest>,
) -> Result<(StatusCode, Json<CreateRoomResponse>), AppError> {
    let now = Instant::now();
    {
        let mut bucket = state.create_bucket.lock().await;
        while let Some(front) = bucket.front() {
            if now.duration_since(*front) > Duration::from_secs(60) {
                bucket.pop_front();
            } else {
                break;
            }
        }
        if bucket.len() >= 30 {
            return Err(AppError::http(
                ErrorCode::SlowConsumer,
                "room creation rate limit",
            ));
        }
        bucket.push_back(now);
    }
    let room_id = util::generate_room_id();
    let seed = rand::thread_rng().gen::<u64>().to_string();
    let player_id = util::generate_player_id();
    let region = req.region.unwrap_or_else(|| state.config.region.clone());
    let sim = SimHandle::spawn(room_id.clone());
    let mut master = Player::new(player_id.clone(), req.name.clone(), Role::Master);
    master.ready = true;
    state
        .lobby
        .create_room(
            room_id.clone(),
            seed.clone(),
            region.clone(),
            req.max_players,
            master,
            sim,
        )
        .await;
    let ws_url = format!("ws://localhost:{}/v1/ws", state.config.ws_port);
    let token = state
        .auth
        .mint_ws_token(&room_id, &player_id, "master", 3600)
        .map_err(AppError::Other)?;
    let response = CreateRoomResponse {
        room_id,
        seed,
        region,
        ws_url,
        ws_token: token,
        role: "master".to_string(),
        state: "lobby".to_string(),
        max_players: req.max_players,
        player_id,
    };
    Ok((StatusCode::CREATED, Json(response)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JoinRequest {
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JoinResponse {
    room_id: String,
    ws_url: String,
    ws_token: String,
    role: String,
    state: String,
    player_id: String,
}

async fn join_room(
    Path(room_id): Path<String>,
    State(state): State<Arc<HttpState>>,
    Json(req): Json<JoinRequest>,
) -> Result<Json<JoinResponse>, AppError> {
    let player_id = util::generate_player_id();
    let player = Player::new(player_id.clone(), req.name.clone(), Role::Member);
    state.lobby.join_room(&room_id, player).await?;
    let ws_url = format!("ws://localhost:{}/v1/ws", state.config.ws_port);
    let token = state
        .auth
        .mint_ws_token(&room_id, &player_id, "member", 3600)
        .map_err(AppError::Other)?;
    Ok(Json(JoinResponse {
        room_id,
        ws_url,
        ws_token: token,
        role: "member".to_string(),
        state: "lobby".to_string(),
        player_id,
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LeaveRequest {
    player_id: String,
}

async fn leave_room(
    Path(room_id): Path<String>,
    State(state): State<Arc<HttpState>>,
    Json(req): Json<LeaveRequest>,
) -> impl IntoResponse {
    state.lobby.leave_room(&room_id, &req.player_id).await;
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartRequest {
    player_id: String,
    #[serde(default)]
    countdown_sec: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartResponse {
    state: String,
    start_at_ms: u64,
}

async fn start_room(
    Path(room_id): Path<String>,
    State(state): State<Arc<HttpState>>,
    Json(req): Json<StartRequest>,
) -> Result<Json<StartResponse>, AppError> {
    let countdown = util::clamp_countdown(req.countdown_sec.unwrap_or(3));
    let payload = state
        .lobby
        .start_room(
            &room_id,
            &req.player_id,
            countdown,
            state.config.require_ready,
        )
        .await?;
    Ok(Json(StartResponse {
        state: "starting".to_string(),
        start_at_ms: payload.start_at_ms,
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadyRequest {
    player_id: String,
    ready: bool,
}

async fn set_ready(
    Path(room_id): Path<String>,
    State(state): State<Arc<HttpState>>,
    Json(req): Json<ReadyRequest>,
) -> Result<Json<LobbyState>, AppError> {
    let lobby_state = state
        .lobby
        .set_ready(&room_id, &req.player_id, req.ready)
        .await?;
    Ok(Json(lobby_state))
}
