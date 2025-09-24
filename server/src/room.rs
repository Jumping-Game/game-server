use crate::{
    errors::{ErrorCode, ServerError},
    protocol::SnapshotPayload,
    sim::{PlayerInputSample, Simulation, SimulationState},
};
use std::collections::{HashMap, VecDeque};

#[derive(Clone)]
pub struct RoomConfig {
    pub max_rollback_ticks: u32,
    pub input_lead_ticks: u32,
}

impl Default for RoomConfig {
    fn default() -> Self {
        Self {
            max_rollback_ticks: 120,
            input_lead_ticks: 2,
        }
    }
}

#[derive(Clone)]
pub struct InputEvent {
    pub tick: u64,
    pub seq: u64,
    pub axis_x: f32,
    pub jump: bool,
}

pub struct Room {
    pub room_id: String,
    simulation: Simulation,
    state: SimulationState,
    inputs: HashMap<String, VecDeque<InputEvent>>,
    config: RoomConfig,
}

impl Room {
    pub fn new(room_id: impl Into<String>, seed: u64, config: RoomConfig) -> Self {
        Self {
            room_id: room_id.into(),
            simulation: Simulation::new(seed),
            state: SimulationState::default(),
            inputs: HashMap::new(),
            config,
        }
    }

    pub fn register_player(&mut self, player_id: &str) {
        self.simulation.ensure_player(&mut self.state, player_id);
        self.inputs.entry(player_id.to_string()).or_default();
    }

    pub fn push_input(&mut self, player_id: &str, event: InputEvent) -> Result<(), ServerError> {
        let window_start = self
            .state
            .tick
            .saturating_sub(self.config.max_rollback_ticks as u64);
        let window_end = self.state.tick + self.config.input_lead_ticks as u64;
        if event.tick < window_start || event.tick > window_end {
            return Err(ServerError::new(
                ErrorCode::InvalidTick,
                "input outside acceptance window",
            ));
        }
        let queue = self.inputs.entry(player_id.to_string()).or_default();
        queue.push_back(event);
        Ok(())
    }

    pub fn step(&mut self) {
        let mut inputs = Vec::new();
        let mut keys: Vec<String> = self.inputs.keys().cloned().collect();
        keys.sort();
        for player_id in keys {
            let queue = self.inputs.get_mut(&player_id).expect("queue present");
            while let Some(front) = queue.front() {
                if front.tick <= self.state.tick + 1 {
                    let front = queue.pop_front().unwrap();
                    inputs.push((
                        player_id.clone(),
                        PlayerInputSample {
                            axis_x: front.axis_x,
                            jump: front.jump,
                            seq: front.seq,
                        },
                    ));
                } else {
                    break;
                }
            }
        }
        self.simulation.step(&mut self.state, &inputs);
    }

    pub fn snapshot(&self, full: bool) -> SnapshotPayload {
        self.simulation.build_snapshot(&self.state, full)
    }

    pub fn tick(&self) -> u64 {
        self.state.tick
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_inputs_in_window() {
        let mut room = Room::new("r", 1, RoomConfig::default());
        room.register_player("p1");
        room.push_input(
            "p1",
            InputEvent {
                tick: 1,
                seq: 1,
                axis_x: 0.5,
                jump: true,
            },
        )
        .unwrap();
        room.step();
        let snap = room.snapshot(true);
        assert_eq!(snap.tick, 1);
        assert_eq!(snap.players.len(), 1);
        assert!(snap.players[0].y > 0.0);
    }
}
