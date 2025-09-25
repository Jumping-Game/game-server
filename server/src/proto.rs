use crate::{
    errors::{ErrorCode, WireError},
    util,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Envelope<T> {
    #[serde(rename = "type")]
    pub kind: String,
    pub pv: u32,
    pub seq: u64,
    pub ts: u64,
    pub payload: T,
}

impl<T> Envelope<T> {
    pub fn new(kind: impl Into<String>, seq: u64, payload: T) -> Self {
        Self {
            kind: kind.into(),
            pv: PROTOCOL_VERSION,
            seq,
            ts: util::now_ms(),
            payload,
        }
    }

    pub fn boxed(kind: impl Into<String>, seq: u64, payload: T) -> Box<Self> {
        Box::new(Self::new(kind, seq, payload))
    }
}

pub fn env<T: DeserializeOwned>(text: &str) -> Result<Envelope<T>, WireError> {
    let envelope: Envelope<T> = serde_json::from_str(text)
        .map_err(|err| WireError::new(ErrorCode::InvalidState, err.to_string()))?;
    if envelope.pv != PROTOCOL_VERSION {
        return Err(WireError::new(
            ErrorCode::BadVersion,
            format!("expected pv={}, got {}", PROTOCOL_VERSION, envelope.pv),
        ));
    }
    Ok(envelope)
}

pub fn send<T: Serialize>(kind: &str, seq: u64, payload: T) -> Result<String, serde_json::Error> {
    let frame = Envelope::new(kind.to_string(), seq, payload);
    serde_json::to_string(&frame)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClientJoin {
    pub name: String,
    #[serde(default)]
    pub client_version: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub capabilities: Option<ClientCapabilities>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClientCapabilities {
    #[serde(default)]
    pub tilt: Option<bool>,
    #[serde(default)]
    pub vibrate: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClientInput {
    pub tick: u64,
    pub axis_x: f32,
    #[serde(default)]
    pub jump: bool,
    #[serde(default)]
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClientInputBatch {
    pub start_tick: u64,
    pub frames: Vec<BatchFrame>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BatchFrame {
    #[serde(default)]
    pub d: u64,
    pub axis_x: f32,
    #[serde(default)]
    pub jump: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClientPing {
    pub t0: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClientReconnect {
    pub player_id: String,
    pub resume_token: String,
    pub last_ack_tick: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClientReadySet {
    pub ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClientStartRequest {
    #[serde(default)]
    pub countdown_sec: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClientCharacterSelect {
    #[serde(default)]
    pub character_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientFrame {
    Join {
        #[serde(flatten)]
        meta: Envelope<ClientJoin>,
    },
    Input {
        #[serde(flatten)]
        meta: Envelope<ClientInput>,
    },
    InputBatch {
        #[serde(flatten)]
        meta: Envelope<ClientInputBatch>,
    },
    Ping {
        #[serde(flatten)]
        meta: Envelope<ClientPing>,
    },
    Reconnect {
        #[serde(flatten)]
        meta: Envelope<ClientReconnect>,
    },
    ReadySet {
        #[serde(flatten)]
        meta: Envelope<ClientReadySet>,
    },
    StartRequest {
        #[serde(flatten)]
        meta: Envelope<ClientStartRequest>,
    },
    CharacterSelect {
        #[serde(flatten)]
        meta: Envelope<ClientCharacterSelect>,
    },
}

impl ClientFrame {
    pub fn seq(&self) -> u64 {
        match self {
            ClientFrame::Join { meta } => meta.seq,
            ClientFrame::Input { meta } => meta.seq,
            ClientFrame::InputBatch { meta } => meta.seq,
            ClientFrame::Ping { meta } => meta.seq,
            ClientFrame::Reconnect { meta } => meta.seq,
            ClientFrame::ReadySet { meta } => meta.seq,
            ClientFrame::StartRequest { meta } => meta.seq,
            ClientFrame::CharacterSelect { meta } => meta.seq,
        }
    }

    pub fn kind(&self) -> &str {
        match self {
            ClientFrame::Join { .. } => "join",
            ClientFrame::Input { .. } => "input",
            ClientFrame::InputBatch { .. } => "input_batch",
            ClientFrame::Ping { .. } => "ping",
            ClientFrame::Reconnect { .. } => "reconnect",
            ClientFrame::ReadySet { .. } => "ready_set",
            ClientFrame::StartRequest { .. } => "start_request",
            ClientFrame::CharacterSelect { .. } => "character_select",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WelcomePayload {
    pub player_id: String,
    pub resume_token: String,
    pub room_id: String,
    pub seed: String,
    pub role: String,
    pub room_state: String,
    pub lobby: LobbyState,
    pub cfg: NetConfig,
    #[serde(default)]
    pub feature_flags: FeatureFlags,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FeatureFlags {
    #[serde(default)]
    pub enemies: bool,
    #[serde(default)]
    pub moving_platforms: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LobbyState {
    pub room_state: String,
    pub players: Vec<LobbyPlayer>,
    #[serde(default)]
    pub max_players: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LobbyPlayer {
    pub id: String,
    pub name: String,
    pub role: String,
    #[serde(default)]
    pub ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub character_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StartCountdownPayload {
    pub start_at_ms: u64,
    pub server_tick: u64,
    pub countdown_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StartPayload {
    pub start_tick: u64,
    pub server_tick: u64,
    pub server_time_ms: u64,
    pub tps: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SnapshotPayload {
    pub tick: u64,
    pub ack_tick: u64,
    pub last_input_seq: u64,
    #[serde(default)]
    pub full: bool,
    pub players: Vec<NetPlayer>,
    #[serde(default)]
    pub events: Vec<NetEvent>,
    pub stats: SnapshotStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SnapshotStats {
    #[serde(default)]
    pub dropped_snapshots: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NetPlayer {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    #[serde(default)]
    pub alive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NetEvent {
    pub kind: String,
    pub x: f32,
    pub y: f32,
    pub tick: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RoleChangedPayload {
    pub new_master_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PongPayload {
    pub t0: u64,
    pub t1: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PlayerPresencePayload {
    pub id: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FinishPayload {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerFrame {
    Welcome {
        #[serde(flatten)]
        meta: Box<Envelope<WelcomePayload>>,
    },
    LobbyState {
        #[serde(flatten)]
        meta: Box<Envelope<LobbyState>>,
    },
    StartCountdown {
        #[serde(flatten)]
        meta: Box<Envelope<StartCountdownPayload>>,
    },
    Start {
        #[serde(flatten)]
        meta: Box<Envelope<StartPayload>>,
    },
    Snapshot {
        #[serde(flatten)]
        meta: Box<Envelope<SnapshotPayload>>,
    },
    RoleChanged {
        #[serde(flatten)]
        meta: Box<Envelope<RoleChangedPayload>>,
    },
    Pong {
        #[serde(flatten)]
        meta: Box<Envelope<PongPayload>>,
    },
    PlayerPresence {
        #[serde(flatten)]
        meta: Box<Envelope<PlayerPresencePayload>>,
    },
    Finish {
        #[serde(flatten)]
        meta: Box<Envelope<FinishPayload>>,
    },
    Error {
        #[serde(flatten)]
        meta: Box<Envelope<WireError>>,
    },
}

impl ServerFrame {
    pub fn seq(&self) -> u64 {
        match self {
            ServerFrame::Welcome { meta } => meta.seq,
            ServerFrame::LobbyState { meta } => meta.seq,
            ServerFrame::StartCountdown { meta } => meta.seq,
            ServerFrame::Start { meta } => meta.seq,
            ServerFrame::Snapshot { meta } => meta.seq,
            ServerFrame::RoleChanged { meta } => meta.seq,
            ServerFrame::Pong { meta } => meta.seq,
            ServerFrame::PlayerPresence { meta } => meta.seq,
            ServerFrame::Finish { meta } => meta.seq,
            ServerFrame::Error { meta } => meta.seq,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NetConfig {
    pub tps: u64,
    pub snapshot_rate_hz: u64,
    pub max_rollback_ticks: u64,
    pub input_lead_ticks: u64,
    pub world: NetWorldCfg,
    pub difficulty: NetDifficultyCfg,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            tps: 60,
            snapshot_rate_hz: 10,
            max_rollback_ticks: 120,
            input_lead_ticks: 2,
            world: NetWorldCfg::default(),
            difficulty: NetDifficultyCfg::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NetWorldCfg {
    pub world_width: u32,
    pub platform_width: u32,
    pub platform_height: u32,
    pub gap_min: u32,
    pub gap_max: u32,
    pub gravity: i32,
    pub jump_vy: i32,
    pub spring_vy: i32,
    pub max_vx: i32,
    pub tilt_accel: i32,
}

impl Default for NetWorldCfg {
    fn default() -> Self {
        Self {
            world_width: 1080,
            platform_width: 120,
            platform_height: 18,
            gap_min: 120,
            gap_max: 240,
            gravity: -2200,
            jump_vy: 1200,
            spring_vy: 1800,
            max_vx: 900,
            tilt_accel: 1200,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NetDifficultyCfg {
    pub gap_min_start: u32,
    pub gap_min_end: u32,
    pub gap_max_start: u32,
    pub gap_max_end: u32,
    pub spring_chance_start: f32,
    pub spring_chance_end: f32,
}

impl Default for NetDifficultyCfg {
    fn default() -> Self {
        Self {
            gap_min_start: 120,
            gap_min_end: 180,
            gap_max_start: 240,
            gap_max_end: 320,
            spring_chance_start: 0.1,
            spring_chance_end: 0.03,
        }
    }
}
