use serde::Serialize;
use std::time::SystemTime;

#[derive(Debug, Serialize, Clone)]
pub struct PresenceEvent {
    pub room_id: String,
    pub player_id: String,
    pub joined: bool,
    pub ts: SystemTime,
}

impl PresenceEvent {
    pub fn join(room_id: impl Into<String>, player_id: impl Into<String>) -> Self {
        Self {
            room_id: room_id.into(),
            player_id: player_id.into(),
            joined: true,
            ts: SystemTime::now(),
        }
    }

    pub fn leave(room_id: impl Into<String>, player_id: impl Into<String>) -> Self {
        Self {
            room_id: room_id.into(),
            player_id: player_id.into(),
            joined: false,
            ts: SystemTime::now(),
        }
    }
}
