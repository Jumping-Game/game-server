use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub const SERVER_PV: u32 = 1;

#[serde_as]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RoomSeed(#[serde_as(as = "DisplayFromStr")] pub u64);

impl RoomSeed {
    pub fn random() -> Self {
        Self(rand::random::<u64>())
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MessageMeta {
    pub pv: u32,
    pub seq: u64,
    pub ts: u64,
}

impl MessageMeta {
    pub fn new(seq: u64) -> Self {
        Self {
            pv: SERVER_PV,
            seq,
            ts: current_ts_millis(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InboundMessage {
    #[serde(rename = "join")]
    Join {
        #[serde(flatten)]
        meta: MessageMeta,
        payload: JoinPayload,
    },
    #[serde(rename = "input")]
    Input {
        #[serde(flatten)]
        meta: MessageMeta,
        payload: InputPayload,
    },
    #[serde(rename = "input_batch")]
    InputBatch {
        #[serde(flatten)]
        meta: MessageMeta,
        payload: InputBatchPayload,
    },
    #[serde(rename = "ping")]
    Ping {
        #[serde(flatten)]
        meta: MessageMeta,
        payload: PingPayload,
    },
    #[serde(rename = "reconnect")]
    Reconnect {
        #[serde(flatten)]
        meta: MessageMeta,
        payload: ReconnectPayload,
    },
}

impl InboundMessage {
    pub fn meta(&self) -> &MessageMeta {
        match self {
            InboundMessage::Join { meta, .. }
            | InboundMessage::Input { meta, .. }
            | InboundMessage::InputBatch { meta, .. }
            | InboundMessage::Ping { meta, .. }
            | InboundMessage::Reconnect { meta, .. } => meta,
        }
    }

    pub fn payload_name(&self) -> &'static str {
        match self {
            InboundMessage::Join { .. } => "join",
            InboundMessage::Input { .. } => "input",
            InboundMessage::InputBatch { .. } => "input_batch",
            InboundMessage::Ping { .. } => "ping",
            InboundMessage::Reconnect { .. } => "reconnect",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutboundMessage {
    #[serde(rename = "welcome")]
    Welcome {
        #[serde(flatten)]
        meta: MessageMeta,
        payload: WelcomePayload,
    },
    #[serde(rename = "start")]
    Start {
        #[serde(flatten)]
        meta: MessageMeta,
        payload: StartPayload,
    },
    #[serde(rename = "snapshot")]
    Snapshot {
        #[serde(flatten)]
        meta: MessageMeta,
        payload: SnapshotPayload,
    },
    #[serde(rename = "pong")]
    Pong {
        #[serde(flatten)]
        meta: MessageMeta,
        payload: PongPayload,
    },
    #[serde(rename = "error")]
    Error {
        #[serde(flatten)]
        meta: MessageMeta,
        payload: ErrorPayload,
    },
    #[serde(rename = "finish")]
    Finish {
        #[serde(flatten)]
        meta: MessageMeta,
        payload: FinishPayload,
    },
    #[serde(rename = "player_presence")]
    PlayerPresence {
        #[serde(flatten)]
        meta: MessageMeta,
        payload: PlayerPresencePayload,
    },
}

impl OutboundMessage {
    pub fn meta(&self) -> &MessageMeta {
        match self {
            OutboundMessage::Welcome { meta, .. }
            | OutboundMessage::Start { meta, .. }
            | OutboundMessage::Snapshot { meta, .. }
            | OutboundMessage::Pong { meta, .. }
            | OutboundMessage::Error { meta, .. }
            | OutboundMessage::Finish { meta, .. }
            | OutboundMessage::PlayerPresence { meta, .. } => meta,
        }
    }

    pub fn new_welcome(seq: u64, payload: WelcomePayload) -> Self {
        OutboundMessage::Welcome {
            meta: MessageMeta::new(seq),
            payload,
        }
    }

    pub fn new_start(seq: u64, payload: StartPayload) -> Self {
        OutboundMessage::Start {
            meta: MessageMeta::new(seq),
            payload,
        }
    }

    pub fn new_snapshot(seq: u64, payload: SnapshotPayload) -> Self {
        OutboundMessage::Snapshot {
            meta: MessageMeta::new(seq),
            payload,
        }
    }

    pub fn new_pong(seq: u64, payload: PongPayload) -> Self {
        OutboundMessage::Pong {
            meta: MessageMeta::new(seq),
            payload,
        }
    }

    pub fn new_error(seq: u64, payload: ErrorPayload) -> Self {
        OutboundMessage::Error {
            meta: MessageMeta::new(seq),
            payload,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct JoinPayload {
    pub name: String,
    pub client_version: String,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub capabilities: HashMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InputPayload {
    pub tick: u64,
    pub axis_x: f32,
    pub jump: bool,
    #[serde(default)]
    pub shoot: bool,
    #[serde(default)]
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InputBatchPayload {
    pub start_tick: u64,
    pub frames: Vec<InputFrame>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InputFrame {
    pub d: u32,
    pub axis_x: f32,
    #[serde(default)]
    pub jump: bool,
    #[serde(default)]
    pub shoot: bool,
    #[serde(default)]
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PingPayload {
    pub t0: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReconnectPayload {
    pub player_id: String,
    pub resume_token: String,
    pub last_ack_tick: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WelcomePayload {
    pub player_id: String,
    pub resume_token: String,
    pub room_id: String,
    pub seed: RoomSeed,
    pub cfg: SessionConfig,
    #[serde(default)]
    pub feature_flags: HashMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SessionConfig {
    pub tps: u32,
    pub snapshot_rate_hz: u32,
    pub max_rollback_ticks: u32,
    pub input_lead_ticks: u32,
    #[serde(default)]
    pub world: Option<WorldConfig>,
    #[serde(default)]
    pub difficulty: Option<DifficultyConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorldConfig {
    pub world_width: f32,
    pub platform_width: f32,
    pub platform_height: f32,
    pub gap_min: f32,
    pub gap_max: f32,
    pub gravity: f32,
    pub jump_vy: f32,
    pub spring_vy: f32,
    pub max_vx: f32,
    pub tilt_accel: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DifficultyConfig {
    pub gap_min_start: f32,
    pub gap_min_end: f32,
    pub gap_max_start: f32,
    pub gap_max_end: f32,
    pub spring_chance_start: f32,
    pub spring_chance_end: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StartPayload {
    pub start_tick: u64,
    pub server_tick: u64,
    pub server_time_ms: u64,
    pub tps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SnapshotPayload {
    pub tick: u64,
    pub ack_tick: u64,
    pub last_input_seq: u64,
    pub full: bool,
    pub players: Vec<PlayerSnapshot>,
    #[serde(default)]
    pub events: Vec<SnapshotEvent>,
    #[serde(default)]
    pub stats: SnapshotStats,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PlayerSnapshot {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    #[serde(default = "default_alive")]
    pub alive: bool,
}

fn default_alive() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SnapshotEvent {
    pub kind: String,
    pub x: f32,
    pub y: f32,
    pub tick: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SnapshotStats {
    #[serde(default)]
    pub dropped_snapshots: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PongPayload {
    pub t0: u64,
    pub t1: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ErrorPayload {
    pub code: crate::errors::ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FinishPayload {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PlayerPresencePayload {
    pub id: String,
    pub state: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Quantized(pub f32);

impl Quantized {
    pub fn from_f64(v: f64) -> Self {
        let rounded = (v * 10.0).round() / 10.0;
        Self(rounded as f32)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlayerInput {
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorldSnapshot {
    pub players: HashMap<String, PlayerSnapshot>,
    pub platforms: Vec<SnapshotEvent>,
}

fn current_ts_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantization_rounds_to_decimal() {
        let q = Quantized::from_f64(1.234);
        assert_eq!(q.0, 1.2);
        let q = Quantized::from_f64(-1.255);
        assert_eq!(q.0, -1.3);
    }
}
