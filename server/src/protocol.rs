use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use std::collections::HashMap;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InboundMessage {
    #[serde(rename = "join")]
    Join(JoinPayload),
    #[serde(rename = "input")]
    Input(InputPayload),
    #[serde(rename = "input_batch")]
    InputBatch(InputBatchPayload),
    #[serde(rename = "ping")]
    Ping(PingPayload),
    #[serde(rename = "reconnect")]
    Reconnect(ReconnectPayload),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutboundMessage {
    #[serde(rename = "welcome")]
    Welcome(WelcomePayload),
    #[serde(rename = "start")]
    Start(StartPayload),
    #[serde(rename = "snapshot")]
    Snapshot(SnapshotPayload),
    #[serde(rename = "pong")]
    Pong(PongPayload),
    #[serde(rename = "error")]
    Error(ErrorPayload),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Envelope<T> {
    pub pv: u32,
    pub seq: u64,
    pub ts: u64,
    pub payload: T,
}

impl<T> Envelope<T> {
    pub fn new(seq: u64, ts: u64, payload: T) -> Self {
        Self {
            pv: SERVER_PV,
            seq,
            ts,
            payload,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct JoinPayload {
    pub room_id: String,
    pub player_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InputPayload {
    pub tick: u64,
    pub seq: u64,
    pub action: PlayerAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InputBatchPayload {
    pub inputs: Vec<InputPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PingPayload {
    pub t0: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReconnectPayload {
    pub player_id: String,
    pub resume_token: String,
    pub last_ack_tick: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WelcomePayload {
    pub player_id: String,
    pub resume_token: String,
    pub seed: RoomSeed,
    pub cfg: SessionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SessionConfig {
    pub tick_rate: u32,
    pub snapshot_rate: u32,
    pub max_rollback_ticks: u32,
    pub input_lead_ticks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StartPayload {
    pub start_tick: u64,
    pub server_tick: u64,
    pub server_time_ms: u64,
    pub tps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SnapshotPayload {
    pub tick: u64,
    pub full: bool,
    pub players: Vec<PlayerSnapshot>,
    pub platforms: Vec<PlatformSnapshot>,
    pub ack_tick: u64,
    pub last_input_seq: u64,
    pub dropped_snapshots: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PlayerSnapshot {
    pub player_id: String,
    pub position_y: Quantized,
    pub velocity_y: Quantized,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PlatformSnapshot {
    pub platform_id: u64,
    pub position_y: Quantized,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PongPayload {
    pub t0: u64,
    pub t1: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ErrorPayload {
    pub code: crate::errors::ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlayerAction {
    Idle,
    Thrust,
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
    pub action: PlayerAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorldSnapshot {
    pub players: HashMap<String, PlayerSnapshot>,
    pub platforms: Vec<PlatformSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WsBootstrap {
    pub room_id: String,
    pub seed: RoomSeed,
    pub ws_url: String,
    pub ws_token: String,
    pub player_id: String,
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
