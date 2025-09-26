use crate::{
    lobby::Room,
    proto::{Envelope, NetPlayer, ServerFrame, SnapshotPayload, SnapshotStats},
};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    sync::mpsc,
    time::{Duration, Instant, MissedTickBehavior},
};

const SNAPSHOT_INTERVAL_MS: u64 = 100;
const FULL_SNAPSHOT_INTERVAL_MS: u64 = 1000;
const MAX_ROLLBACK_TICKS: u64 = 120;
const INPUT_LEAD_TICKS: u64 = 2;

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
                                let min_tick = tick.saturating_sub(MAX_ROLLBACK_TICKS);
                                let max_tick = tick + INPUT_LEAD_TICKS;
                                if input_tick < min_tick || input_tick > max_tick {
                                    continue;
                                }
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
                        emit_snapshot(
                            &room_ref,
                            &mut players,
                            tick,
                            &mut force_full,
                            &mut last_full_at,
                            &mut last_snapshot,
                        ).await;
                    }
                    SimCommand::ForceSnapshot => {
                        force_full = true;
                        if running {
                            emit_snapshot(
                                &room_ref,
                                &mut players,
                                tick,
                                &mut force_full,
                                &mut last_full_at,
                                &mut last_snapshot,
                            )
                            .await;
                        }
                    }
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

                let snapshot_due = last_snapshot.elapsed() >= Duration::from_millis(SNAPSHOT_INTERVAL_MS);
                if snapshot_due {
                    emit_snapshot(
                        &room_ref,
                        &mut players,
                        tick,
                        &mut force_full,
                        &mut last_full_at,
                        &mut last_snapshot,
                    ).await;
                }
            }
        }
    }
}

async fn emit_snapshot(
    room_ref: &Option<std::sync::Weak<tokio::sync::RwLock<Room>>>,
    players: &mut HashMap<String, SimPlayer>,
    tick: u64,
    force_full: &mut bool,
    last_full_at: &mut Instant,
    last_snapshot: &mut Instant,
) {
    let Some(room) = room_ref.as_ref().and_then(|r| r.upgrade()) else {
        return;
    };
    let mut room_guard = room.write().await;
    let seq = room_guard.next_seq();
    let mut net_players = Vec::with_capacity(room_guard.players.len());
    let mut max_input_seq = 0;
    for player in room_guard.players.iter_mut() {
        if let Some(state) = players.get(&player.id) {
            player.last_ack_tick = state.last_ack_tick;
            player.last_input_seq = state.last_input_seq;
            max_input_seq = max_input_seq.max(state.last_input_seq);
            net_players.push(NetPlayer {
                id: player.id.clone(),
                x: state.x,
                y: state.y,
                vx: state.vx,
                vy: state.vy,
                alive: true,
                character_id: player.character_id.clone(),
            });
        } else {
            net_players.push(NetPlayer {
                id: player.id.clone(),
                x: 0.0,
                y: 0.0,
                vx: 0.0,
                vy: 0.0,
                alive: true,
                character_id: player.character_id.clone(),
            });
        }
    }
    let full_due =
        *force_full || last_full_at.elapsed() >= Duration::from_millis(FULL_SNAPSHOT_INTERVAL_MS);
    let payload = SnapshotPayload {
        tick,
        ack_tick: tick,
        last_input_seq: max_input_seq,
        full: full_due,
        players: net_players,
        events: Vec::new(),
        stats: SnapshotStats {
            dropped_snapshots: 0,
        },
    };
    let frame = ServerFrame::Snapshot {
        meta: Envelope::boxed("snapshot", seq, payload),
    };
    room_guard.broadcast(frame).await;
    *last_snapshot = Instant::now();
    if full_due {
        *force_full = false;
        *last_full_at = Instant::now();
    }
}
