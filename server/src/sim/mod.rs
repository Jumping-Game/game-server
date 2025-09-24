pub mod fixed;
pub mod physics;
pub mod rng;
pub mod snapshot;
pub mod world;

use crate::protocol::{PlatformSnapshot, PlayerAction, PlayerSnapshot, Quantized, SnapshotPayload};
use fixed::Fixed;
use physics::PhysicsWorld;
use snapshot::SnapshotBuilder;
use std::collections::HashMap;
use world::WorldGenerator;

#[derive(Debug, Clone)]
pub struct PlayerSimState {
    pub player_id: String,
    pub position_y: Fixed,
    pub velocity_y: Fixed,
    pub last_input_seq: u64,
}

#[derive(Debug, Clone)]
pub struct SimulationConfig {
    pub tick_rate: u32,
    pub gravity: Fixed,
    pub thrust: Fixed,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            tick_rate: 60,
            gravity: Fixed::from_f64(-0.1),
            thrust: Fixed::from_f64(0.2),
        }
    }
}

#[derive(Debug, Default)]
pub struct SimulationState {
    pub tick: u64,
    pub players: HashMap<String, PlayerSimState>,
}

pub struct Simulation {
    pub config: SimulationConfig,
    pub world: WorldGenerator,
    pub physics: PhysicsWorld,
    pub snapshot_builder: SnapshotBuilder,
}

impl Simulation {
    pub fn new(seed: u64) -> Self {
        Self {
            config: SimulationConfig::default(),
            world: WorldGenerator::new(seed),
            physics: PhysicsWorld::new(),
            snapshot_builder: SnapshotBuilder::new(),
        }
    }

    pub fn step(&self, state: &mut SimulationState, inputs: &[(String, PlayerAction, u64)]) {
        state.tick += 1;
        for (player_id, action, seq) in inputs {
            let entry = state
                .players
                .entry(player_id.clone())
                .or_insert_with(|| PlayerSimState {
                    player_id: player_id.clone(),
                    position_y: Fixed::ZERO,
                    velocity_y: Fixed::ZERO,
                    last_input_seq: 0,
                });
            if entry.last_input_seq < *seq {
                entry.last_input_seq = *seq;
            }
            self.physics.apply_action(entry, action, &self.config);
        }
        for player in state.players.values_mut() {
            self.physics.tick_player(player, &self.config);
        }
    }

    pub fn build_snapshot(&self, state: &SimulationState, full: bool) -> SnapshotPayload {
        let platforms = self.world.platforms_for_tick(state.tick);
        let mut players: Vec<PlayerSnapshot> = state
            .players
            .values()
            .map(|p| PlayerSnapshot {
                player_id: p.player_id.clone(),
                position_y: Quantized::from_f64(p.position_y.to_f64()),
                velocity_y: Quantized::from_f64(p.velocity_y.to_f64()),
            })
            .collect();
        players.sort_by(|a, b| a.player_id.cmp(&b.player_id));
        let platform_snaps: Vec<PlatformSnapshot> = platforms
            .into_iter()
            .map(|(id, pos)| PlatformSnapshot {
                platform_id: id,
                position_y: Quantized::from_f64(pos.to_f64()),
            })
            .collect();
        self.snapshot_builder.build(
            state.tick,
            players,
            platform_snaps,
            full,
            state
                .players
                .values()
                .map(|p| (p.player_id.clone(), p.last_input_seq))
                .collect(),
        )
    }

    pub fn ensure_player(&self, state: &mut SimulationState, player_id: &str) {
        state
            .players
            .entry(player_id.to_string())
            .or_insert_with(|| PlayerSimState {
                player_id: player_id.to_string(),
                position_y: Fixed::ZERO,
                velocity_y: Fixed::ZERO,
                last_input_seq: 0,
            });
    }
}
