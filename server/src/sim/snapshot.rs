use crate::protocol::{PlayerSnapshot, SnapshotPayload};
use std::collections::HashMap;

pub struct SnapshotBuilder;

impl SnapshotBuilder {
    pub fn new() -> Self {
        Self
    }

    pub fn build(
        &self,
        tick: u64,
        players: Vec<PlayerSnapshot>,
        platforms: Vec<crate::protocol::PlatformSnapshot>,
        full: bool,
        input_seq: HashMap<String, u64>,
    ) -> SnapshotPayload {
        let ack_tick = tick.saturating_sub(1);
        let last_input_seq = input_seq.values().copied().max().unwrap_or(0);
        SnapshotPayload {
            tick,
            full,
            players,
            platforms,
            ack_tick,
            last_input_seq,
            dropped_snapshots: 0,
        }
    }
}
