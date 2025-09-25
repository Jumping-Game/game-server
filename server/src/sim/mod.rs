use crate::protocol::{PlayerSnapshot, SnapshotPayload, SnapshotStats};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct PlayerSimState {
    pub player_id: String,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub last_input_seq: u64,
}

#[derive(Debug, Default)]
pub struct SimulationState {
    pub tick: u64,
    pub players: HashMap<String, PlayerSimState>,
}

#[derive(Debug, Clone, Copy)]
pub struct PlayerInputSample {
    pub axis_x: f32,
    pub jump: bool,
    pub seq: u64,
}

pub struct Simulation;

impl Simulation {
    pub fn new(_seed: u64) -> Self {
        Self
    }

    pub fn step(&self, state: &mut SimulationState, inputs: &[(String, PlayerInputSample)]) {
        state.tick = state.tick.saturating_add(1);
        for player in state.players.values_mut() {
            player.vy = (player.vy - 200.0).max(-2200.0);
            player.y = (player.y + player.vy / 60.0).max(0.0);
            player.x = player.x.clamp(0.0, 1080.0);
        }
        for (player_id, sample) in inputs {
            let entry = state
                .players
                .entry(player_id.clone())
                .or_insert_with(|| PlayerSimState {
                    player_id: player_id.clone(),
                    ..Default::default()
                });
            entry.vx = (sample.axis_x * 900.0).clamp(-900.0, 900.0);
            entry.x = (entry.x + entry.vx / 60.0).clamp(0.0, 1080.0);
            if sample.jump {
                entry.vy = 1200.0;
                entry.y = (entry.y + entry.vy / 60.0).max(0.0);
            }
            if entry.last_input_seq < sample.seq {
                entry.last_input_seq = sample.seq;
            }
        }
    }

    pub fn build_snapshot(
        &self,
        state: &SimulationState,
        full: bool,
        player_id: Option<&str>,
    ) -> SnapshotPayload {
        let mut players: Vec<PlayerSnapshot> = state
            .players
            .values()
            .map(|p| PlayerSnapshot {
                id: p.player_id.clone(),
                x: p.x,
                y: p.y,
                vx: p.vx,
                vy: p.vy,
                alive: true,
            })
            .collect();
        players.sort_by(|a, b| a.id.cmp(&b.id));
        let last_input_seq = state
            .players
            .values()
            .map(|p| p.last_input_seq)
            .max()
            .unwrap_or(0);
        let ack_tick = state.tick;
        let player_ack_seq = player_id
            .and_then(|id| state.players.get(id))
            .map(|p| p.last_input_seq)
            .unwrap_or(last_input_seq);
        SnapshotPayload {
            tick: state.tick,
            ack_tick,
            last_input_seq: player_ack_seq,
            full,
            players,
            events: Vec::new(),
            stats: SnapshotStats::default(),
        }
    }

    pub fn ensure_player(&self, state: &mut SimulationState, player_id: &str) {
        state
            .players
            .entry(player_id.to_string())
            .or_insert_with(|| PlayerSimState {
                player_id: player_id.to_string(),
                ..Default::default()
            });
    }

    pub fn remove_player(&self, state: &mut SimulationState, player_id: &str) {
        state.players.remove(player_id);
    }
}
