use crate::{
    lobby::Room,
    proto::{Envelope, NetPlayer, ServerFrame, SnapshotPayload, SnapshotStats},
};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    sync::mpsc,
    time::{Duration, Instant, MissedTickBehavior},
};

#[derive(Clone)]
pub struct SimHandle {
    tx: mpsc::Sender<SimCommand>,
}

impl SimHandle {
    pub fn spawn(room_id: String) -> Self {
        let (tx, rx) = mpsc::channel(128);
        tokio::spawn(sim_loop(room_id, rx));
        Self { tx }
    }

    pub async fn attach_room(&self, room: &Arc<tokio::sync::RwLock<Room>>) {
        let _ = self
            .tx
            .send(SimCommand::AttachRoom(Arc::downgrade(room)))
            .await;
    }

    pub async fn player_joined(&self, player_id: String) {
        let _ = self.tx.send(SimCommand::PlayerJoined { player_id }).await;
    }

    pub async fn player_left(&self, player_id: String) {
        let _ = self.tx.send(SimCommand::PlayerLeft { player_id }).await;
    }

    pub async fn submit_input(
        &self,
        player_id: String,
        tick: u64,
        axis_x: f32,
        jump: bool,
        seq: u64,
    ) {
        let _ = self
            .tx
            .send(SimCommand::SubmitInput {
                player_id,
                tick,
                axis_x,
                jump,
                seq,
            })
            .await;
    }

    pub async fn start(&self, start_tick: u64) {
        let _ = self.tx.send(SimCommand::Start { start_tick }).await;
    }

    pub async fn force_full_snapshot(&self) {
        let _ = self.tx.send(SimCommand::ForceSnapshot).await;
    }
}

enum SimCommand {
    AttachRoom(std::sync::Weak<tokio::sync::RwLock<Room>>),
    PlayerJoined {
        player_id: String,
    },
    PlayerLeft {
        player_id: String,
    },
    SubmitInput {
        player_id: String,
        tick: u64,
        axis_x: f32,
        jump: bool,
        seq: u64,
    },
    Start {
        start_tick: u64,
    },
    ForceSnapshot,
}

struct SimPlayer {
    axis_x: f32,
    jump: bool,
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    last_input_seq: u64,
    last_ack_tick: u64,
}

impl SimPlayer {
    fn new() -> Self {
        Self {
            axis_x: 0.0,
            jump: false,
            x: 0.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
            last_input_seq: 0,
            last_ack_tick: 0,
        }
    }
}

async fn sim_loop(_room_id: String, mut rx: mpsc::Receiver<SimCommand>) {
    let mut room_ref: Option<std::sync::Weak<tokio::sync::RwLock<Room>>> = None;
    let mut players: HashMap<String, SimPlayer> = HashMap::new();
    let mut running = false;
    let mut tick: u64 = 0;
    let mut force_full = true;
    let mut last_full_at = Instant::now();
    let mut last_snapshot = Instant::now();
    let mut tick_interval = tokio::time::interval(Duration::from_micros(16_666));
    tick_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            Some(cmd) = rx.recv() => {
                match cmd {
                    SimCommand::AttachRoom(room) => room_ref = Some(room),
                    SimCommand::PlayerJoined { player_id } => {
                        players.entry(player_id).or_insert_with(SimPlayer::new);
                    }
                    SimCommand::PlayerLeft { player_id } => {
                        players.remove(&player_id);
                    }
                    SimCommand::SubmitInput { player_id, tick: input_tick, axis_x, jump, seq } => {
                        if running {
                            if let Some(state) = players.get_mut(&player_id) {
                                state.axis_x = axis_x.clamp(-1.0, 1.0);
                                state.jump = jump;
                                state.last_input_seq = seq;
                                state.last_ack_tick = input_tick;
                            }
                        }
                    }
                    SimCommand::Start { start_tick: start_at } => {
                        running = true;
                        tick = start_at;
                        force_full = true;
                        last_snapshot = Instant::now();
                        last_full_at = Instant::now();
                    }
                    SimCommand::ForceSnapshot => force_full = true,
                }
            }
            _ = tick_interval.tick(), if running => {
                tick += 1;
                // basic physics
                for player in players.values_mut() {
                    player.vx = player.axis_x * 900.0;
                    player.vy = if player.jump { 1200.0 } else { player.vy - 16.0 };
                    player.x += player.vx / 60.0;
                    player.y += (player.vy / 60.0).max(0.0);
                    player.jump = false;
                    player.last_ack_tick = tick;
                }

                let snapshot_due = last_snapshot.elapsed() >= Duration::from_millis(100);
                if snapshot_due {
                    let is_full = force_full || last_full_at.elapsed() >= Duration::from_secs(1);
                    if let Some(room) = room_ref.as_ref().and_then(|r| r.upgrade()) {
                        let mut room_guard = room.write().await;
                        let seq = room_guard.next_seq();
                        let mut net_players = Vec::with_capacity(room_guard.players.len());
                        for player in room_guard.players.iter_mut() {
                            let sim_state = players.get(&player.id);
                            if let Some(state) = sim_state {
                                player.last_ack_tick = state.last_ack_tick;
                                player.last_input_seq = state.last_input_seq;
                            }
                            let (x, y, vx, vy) = if let Some(state) = sim_state {
                                (state.x, state.y, state.vx, state.vy)
                            } else {
                                (0.0, 0.0, 0.0, 0.0)
                            };
                            net_players.push(NetPlayer {
                                id: player.id.clone(),
                                x,
                                y,
                                vx,
                                vy,
                                alive: true,
                            });
                        }
                        let payload = SnapshotPayload {
                            tick,
                            ack_tick: tick,
                            last_input_seq: players.values().map(|p| p.last_input_seq).max().unwrap_or(0),
                            full: is_full,
                            players: net_players,
                            events: Vec::new(),
                            stats: SnapshotStats { dropped_snapshots: 0 },
                        };
                        let frame = ServerFrame::Snapshot {
                            meta: Envelope::boxed("snapshot", seq, payload),
                        };
                        room_guard.broadcast(frame).await;
                    }
                    last_snapshot = Instant::now();
                    if force_full {
                        last_full_at = Instant::now();
                        force_full = false;
                    }
                }
            }
        }
    }
}
