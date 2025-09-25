use crate::{
    errors::{ErrorCode, ServerError},
    protocol::SnapshotPayload,
    sim::{PlayerInputSample, Simulation, SimulationState},
};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{watch, Mutex};
use tokio::time::{self, Duration, Instant};

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
        if !(event.axis_x >= -1.25 && event.axis_x <= 1.25) {
            return Err(ServerError::new(
                ErrorCode::CheatDetected,
                "axis_x out of range",
            ));
        }
        let queue = self.inputs.entry(player_id.to_string()).or_default();
        if let Some(last) = queue.back() {
            if event.tick + 1 < last.tick {
                return Err(ServerError::new(
                    ErrorCode::CheatDetected,
                    "inputs arrived out of order",
                ));
            }
        }
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

    pub fn snapshot_for_player(&self, player_id: &str, full: bool) -> SnapshotPayload {
        self.simulation
            .build_snapshot(&self.state, full, Some(player_id))
    }

    pub fn tick(&self) -> u64 {
        self.state.tick
    }

    pub fn deregister_player(&mut self, player_id: &str) {
        self.inputs.remove(player_id);
        self.simulation.remove_player(&mut self.state, player_id);
    }
}

#[derive(Clone, Debug, Default)]
pub struct SnapshotSignal {
    pub tick: u64,
    pub full: bool,
}

pub struct RoomRuntime {
    room: Mutex<Room>,
    snapshot_tx: watch::Sender<SnapshotSignal>,
    tick_duration: Duration,
    snapshot_interval_ticks: u64,
    full_snapshot_interval_ticks: u64,
    shutdown_tx: watch::Sender<bool>,
}

impl RoomRuntime {
    pub fn new(
        room_id: impl Into<String>,
        seed: u64,
        config: RoomConfig,
        tick_rate: u32,
        snapshot_rate: u32,
    ) -> Arc<Self> {
        let room = Room::new(room_id, seed, config.clone());
        let snapshot_interval_ticks = (tick_rate / snapshot_rate.max(1)) as u64;
        let full_snapshot_interval_ticks = tick_rate as u64;
        let (snapshot_tx, _) = watch::channel(SnapshotSignal {
            tick: 0,
            full: true,
        });
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let runtime = Arc::new(Self {
            room: Mutex::new(room),
            snapshot_tx,
            tick_duration: Duration::from_secs_f64(1.0 / tick_rate.max(1) as f64),
            snapshot_interval_ticks: snapshot_interval_ticks.max(1),
            full_snapshot_interval_ticks: full_snapshot_interval_ticks.max(1),
            shutdown_tx,
        });
        Self::spawn_loop(runtime.clone(), shutdown_rx);
        runtime
    }

    fn spawn_loop(runtime: Arc<Self>, mut shutdown_rx: watch::Receiver<bool>) {
        tokio::spawn(async move {
            let mut ticker = time::interval_at(Instant::now(), runtime.tick_duration);
            ticker.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let mut guard = runtime.room.lock().await;
                        guard.step();
                        let tick = guard.tick();
                        let should_snapshot = tick % runtime.snapshot_interval_ticks == 0;
                        let full = tick % runtime.full_snapshot_interval_ticks == 0;
                        drop(guard);
                        if should_snapshot {
                            let _ = runtime.snapshot_tx.send(SnapshotSignal { tick, full });
                        }
                    }
                    changed = shutdown_rx.changed() => {
                        if changed.is_ok() && *shutdown_rx.borrow() {
                            break;
                        }
                    }
                }
            }
        });
    }

    pub async fn register_player(&self, player_id: &str) {
        let mut guard = self.room.lock().await;
        guard.register_player(player_id);
    }

    pub async fn deregister_player(&self, player_id: &str) {
        let mut guard = self.room.lock().await;
        guard.deregister_player(player_id);
    }

    pub async fn push_input(&self, player_id: &str, event: InputEvent) -> Result<(), ServerError> {
        let mut guard = self.room.lock().await;
        guard.push_input(player_id, event)
    }

    pub async fn snapshot_for_player(&self, player_id: &str, full: bool) -> SnapshotPayload {
        let guard = self.room.lock().await;
        guard.snapshot_for_player(player_id, full)
    }

    pub fn subscribe(&self) -> watch::Receiver<SnapshotSignal> {
        self.snapshot_tx.subscribe()
    }

    pub async fn latest_full_snapshot(&self, player_id: &str) -> SnapshotPayload {
        let guard = self.room.lock().await;
        guard.snapshot_for_player(player_id, true)
    }

    pub async fn tick(&self) -> u64 {
        let guard = self.room.lock().await;
        guard.tick()
    }

    pub async fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
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
        let snap = room.snapshot_for_player("p1", true);
        assert_eq!(snap.tick, 1);
        assert_eq!(snap.players.len(), 1);
        assert!(snap.players[0].y > 0.0);
    }
}
