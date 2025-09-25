use crate::{
    broadcaster::ConnectionQueue,
    errors::{AppError, ErrorCode},
    proto::{
        Envelope, LobbyPlayer, LobbyState, NetConfig, RoleChangedPayload, ServerFrame,
        StartCountdownPayload, StartPayload, WelcomePayload,
    },
    sim::SimHandle,
    util,
};
use std::{collections::HashMap, sync::Arc};
use tokio::{sync::RwLock, task::JoinHandle, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomState {
    Lobby,
    Starting {
        start_at_ms: u64,
        countdown_sec: u64,
    },
    Running {
        start_tick: u64,
    },
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    Master,
    Member,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Master => "master",
            Role::Member => "member",
        }
    }
}

#[derive(Clone)]
pub struct Player {
    pub id: String,
    pub name: String,
    pub role: Role,
    pub ready: bool,
    pub queue: Option<ConnectionQueue>,
    pub last_ack_tick: u64,
    pub last_input_seq: u64,
    pub resume_token: String,
    pub joined_at: u64,
}

impl Player {
    pub fn new(id: String, name: String, role: Role) -> Self {
        Self {
            id,
            name,
            role,
            ready: false,
            queue: None,
            last_ack_tick: 0,
            last_input_seq: 0,
            resume_token: util::generate_resume_token(),
            joined_at: util::now_ms(),
        }
    }
}

pub struct Room {
    pub id: String,
    pub seed: String,
    pub region: String,
    pub max_players: u32,
    pub created_at: u64,
    pub state: RoomState,
    pub players: Vec<Player>,
    pub sim: SimHandle,
    pub seq: std::sync::atomic::AtomicU64,
    countdown_task: Option<JoinHandle<()>>,
}

impl Room {
    pub fn new(id: String, seed: String, region: String, max_players: u32, sim: SimHandle) -> Self {
        Self {
            id,
            seed,
            region,
            max_players,
            created_at: util::now_ms(),
            state: RoomState::Lobby,
            players: Vec::new(),
            sim,
            seq: std::sync::atomic::AtomicU64::new(1),
            countdown_task: None,
        }
    }

    pub fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn add_player(&mut self, player: Player) -> Result<(), AppError> {
        if self.players.len() as u32 >= self.max_players {
            return Err(AppError::http(ErrorCode::RoomStateInvalid, "room full"));
        }
        self.players.push(player);
        Ok(())
    }

    pub fn remove_player(&mut self, player_id: &str) -> Option<Player> {
        if let Some(idx) = self.players.iter().position(|p| p.id == player_id) {
            Some(self.players.remove(idx))
        } else {
            None
        }
    }

    pub fn lobby_state(&self) -> LobbyState {
        LobbyState {
            room_state: match &self.state {
                RoomState::Lobby => "lobby".to_string(),
                RoomState::Starting { .. } => "starting".to_string(),
                RoomState::Running { .. } => "running".to_string(),
                RoomState::Finished => "finished".to_string(),
            },
            players: self
                .players
                .iter()
                .map(|p| LobbyPlayer {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    role: p.role.as_str().to_string(),
                    ready: p.ready,
                })
                .collect(),
            max_players: self.max_players,
        }
    }

    pub fn find_player_mut(&mut self, player_id: &str) -> Option<&mut Player> {
        self.players.iter_mut().find(|p| p.id == player_id)
    }

    pub fn master_id(&self) -> Option<String> {
        self.players
            .iter()
            .find(|p| p.role == Role::Master)
            .map(|p| p.id.clone())
    }

    pub fn ensure_master(&self, player_id: &str) -> Result<(), AppError> {
        let Some(player) = self.players.iter().find(|p| p.id == player_id) else {
            return Err(AppError::http(
                ErrorCode::Unauthorized,
                "player not in room",
            ));
        };
        if player.role != Role::Master {
            return Err(AppError::http(ErrorCode::NotMaster, "master required"));
        }
        Ok(())
    }

    pub async fn broadcast(&self, frame: ServerFrame) {
        for player in &self.players {
            if let Some(queue) = &player.queue {
                queue.push(frame.clone()).await;
            }
        }
    }

    pub fn welcome_payload(&self, player: &Player) -> WelcomePayload {
        WelcomePayload {
            player_id: player.id.clone(),
            resume_token: player.resume_token.clone(),
            room_id: self.id.clone(),
            seed: self.seed.clone(),
            role: player.role.as_str().to_string(),
            room_state: self.state_name(),
            lobby: self.lobby_state(),
            cfg: NetConfig::default(),
            feature_flags: Default::default(),
        }
    }

    pub fn state_name(&self) -> String {
        match &self.state {
            RoomState::Lobby => "lobby".to_string(),
            RoomState::Starting { .. } => "starting".to_string(),
            RoomState::Running { .. } => "running".to_string(),
            RoomState::Finished => "finished".to_string(),
        }
    }

    pub fn set_ready(&mut self, player_id: &str, ready: bool) -> Result<(), AppError> {
        let Some(player) = self.find_player_mut(player_id) else {
            return Err(AppError::http(
                ErrorCode::Unauthorized,
                "player not in room",
            ));
        };
        player.ready = ready;
        Ok(())
    }

    pub fn attach_queue(
        &mut self,
        player_id: &str,
        queue: ConnectionQueue,
    ) -> Result<&mut Player, AppError> {
        let Some(player) = self.find_player_mut(player_id) else {
            return Err(AppError::http(
                ErrorCode::Unauthorized,
                "player not in room",
            ));
        };
        player.queue = Some(queue);
        Ok(player)
    }

    pub fn detach_queue(&mut self, player_id: &str) {
        if let Some(player) = self.find_player_mut(player_id) {
            player.queue = None;
        }
    }

    pub fn everyone_ready(&self) -> bool {
        self.players.iter().all(|p| p.ready)
    }

    pub fn start_countdown(
        &mut self,
        countdown_sec: u64,
    ) -> Result<StartCountdownPayload, AppError> {
        match self.state {
            RoomState::Lobby => {
                let start_at = util::now_ms() + countdown_sec * 1000;
                self.state = RoomState::Starting {
                    start_at_ms: start_at,
                    countdown_sec,
                };
                Ok(StartCountdownPayload {
                    start_at_ms: start_at,
                    server_tick: 0,
                    countdown_sec,
                })
            }
            RoomState::Starting { .. } => Err(AppError::http(
                ErrorCode::CountdownActive,
                "countdown active",
            )),
            RoomState::Running { .. } => {
                Err(AppError::http(ErrorCode::StartAlready, "already running"))
            }
            RoomState::Finished => Err(AppError::http(ErrorCode::StartAlready, "match finished")),
        }
    }

    pub fn set_running(&mut self) -> StartPayload {
        self.state = RoomState::Running { start_tick: 0 };
        StartPayload {
            start_tick: 0,
            server_tick: 0,
            server_time_ms: util::now_ms(),
            tps: 60,
        }
    }

    pub fn transfer_master(&mut self) -> Option<String> {
        let mut candidates: Vec<_> = self
            .players
            .iter_mut()
            .filter(|p| p.role == Role::Member)
            .collect();
        candidates.sort_by_key(|p| p.joined_at);
        if let Some(new_master) = candidates.first_mut() {
            new_master.role = Role::Master;
            Some(new_master.id.clone())
        } else {
            None
        }
    }
}

#[derive(Clone, Default)]
pub struct Lobby {
    pub rooms: Arc<RwLock<HashMap<String, Arc<RwLock<Room>>>>>,
}

pub struct AttachResult {
    pub welcome: ServerFrame,
    pub lobby: ServerFrame,
    pub sim: SimHandle,
    pub resume_token: String,
}

impl Lobby {
    pub fn new() -> Self {
        Self {
            rooms: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn create_room(
        &self,
        id: String,
        seed: String,
        region: String,
        max_players: u32,
        master: Player,
        sim: SimHandle,
    ) -> Arc<RwLock<Room>> {
        let room = Arc::new(RwLock::new(Room::new(
            id.clone(),
            seed,
            region,
            max_players,
            sim.clone(),
        )));
        {
            let mut inner = room.write().await;
            inner.players.push(master.clone());
        }
        sim.attach_room(&room).await;
        sim.player_joined(master.id.clone()).await;
        self.rooms.write().await.insert(id, room.clone());
        room
    }

    pub async fn room(&self, id: &str) -> Option<Arc<RwLock<Room>>> {
        self.rooms.read().await.get(id).cloned()
    }

    pub async fn remove_room(&self, id: &str) {
        self.rooms.write().await.remove(id);
    }

    pub async fn join_room(&self, room_id: &str, mut player: Player) -> Result<(), AppError> {
        let Some(room_arc) = self.room(room_id).await else {
            return Err(AppError::http(
                ErrorCode::RoomStateInvalid,
                "room not found",
            ));
        };
        let mut room = room_arc.write().await;
        match room.state {
            RoomState::Lobby => {}
            RoomState::Starting { .. } | RoomState::Running { .. } | RoomState::Finished => {
                return Err(AppError::http(
                    ErrorCode::RoomStateInvalid,
                    "cannot join now",
                ));
            }
        }
        player.role = Role::Member;
        room.add_player(player.clone())?;
        let seq = room.next_seq();
        let state = room.lobby_state();
        let frame = ServerFrame::LobbyState {
            meta: Envelope::boxed("lobby_state", seq, state),
        };
        let sim = room.sim.clone();
        let player_id = player.id.clone();
        room.broadcast(frame).await;
        drop(room);
        sim.player_joined(player_id).await;
        Ok(())
    }

    pub async fn leave_room(&self, room_id: &str, player_id: &str) {
        if let Some(room_arc) = self.room(room_id).await {
            let mut room = room_arc.write().await;
            let mut cleanup = room.players.is_empty();
            if let Some(removed) = room.remove_player(player_id) {
                cleanup = room.players.is_empty();
                let sim = room.sim.clone();
                let mut frames = Vec::new();
                if removed.role == Role::Master {
                    if let Some(new_master_id) = room.transfer_master() {
                        let seq = room.next_seq();
                        frames.push(ServerFrame::RoleChanged {
                            meta: Envelope::boxed(
                                "role_changed",
                                seq,
                                RoleChangedPayload {
                                    new_master_id: new_master_id.clone(),
                                },
                            ),
                        });
                    }
                }
                let seq = room.next_seq();
                frames.push(ServerFrame::LobbyState {
                    meta: Envelope::boxed("lobby_state", seq, room.lobby_state()),
                });
                for frame in frames.clone() {
                    room.broadcast(frame).await;
                }
                drop(room);
                sim.player_left(removed.id.clone()).await;
            } else {
                drop(room);
            }
            if cleanup {
                self.remove_room(room_id).await;
            }
        }
    }

    pub async fn set_ready(
        &self,
        room_id: &str,
        player_id: &str,
        ready: bool,
    ) -> Result<LobbyState, AppError> {
        let Some(room_arc) = self.room(room_id).await else {
            return Err(AppError::http(
                ErrorCode::RoomStateInvalid,
                "room not found",
            ));
        };
        let mut room = room_arc.write().await;
        room.set_ready(player_id, ready)?;
        let state = room.lobby_state();
        let seq = room.next_seq();
        let frame = ServerFrame::LobbyState {
            meta: Envelope::boxed("lobby_state", seq, state.clone()),
        };
        room.broadcast(frame).await;
        Ok(state)
    }

    pub async fn start_room(
        &self,
        room_id: &str,
        player_id: &str,
        countdown_sec: u64,
        require_ready: bool,
    ) -> Result<StartCountdownPayload, AppError> {
        let Some(room_arc) = self.room(room_id).await else {
            return Err(AppError::http(
                ErrorCode::RoomStateInvalid,
                "room not found",
            ));
        };
        let mut room = room_arc.write().await;
        room.ensure_master(player_id)?;
        if require_ready && !room.everyone_ready() {
            return Err(AppError::http(ErrorCode::RoomNotReady, "players not ready"));
        }
        if let Some(existing) = room.countdown_task.take() {
            existing.abort();
        }
        let payload = room.start_countdown(countdown_sec)?;
        let seq = room.next_seq();
        let frame = ServerFrame::StartCountdown {
            meta: Envelope::boxed("start_countdown", seq, payload.clone()),
        };
        let sim = room.sim.clone();
        let start_at = payload.start_at_ms;
        room.broadcast(frame).await;
        let room_weak = Arc::downgrade(&room_arc);
        room.countdown_task = Some(tokio::spawn(async move {
            let now = util::now_ms();
            if start_at > now {
                tokio::time::sleep(Duration::from_millis(start_at - now)).await;
            }
            if let Some(room_arc) = room_weak.upgrade() {
                let mut guard = room_arc.write().await;
                let start_payload = guard.set_running();
                let seq = guard.next_seq();
                let frame = ServerFrame::Start {
                    meta: Envelope::boxed("start", seq, start_payload.clone()),
                };
                guard.broadcast(frame).await;
                drop(guard);
                sim.start(start_payload.start_tick).await;
                sim.force_full_snapshot().await;
            }
        }));
        Ok(payload)
    }

    pub async fn attach_connection(
        &self,
        room_id: &str,
        player_id: &str,
        queue: ConnectionQueue,
    ) -> Result<AttachResult, AppError> {
        let Some(room_arc) = self.room(room_id).await else {
            return Err(AppError::http(
                ErrorCode::RoomStateInvalid,
                "room not found",
            ));
        };
        let mut room = room_arc.write().await;
        let (player_id_owned, resume_token_owned, role_name);
        {
            let player = room.attach_queue(player_id, queue)?;
            player_id_owned = player.id.clone();
            resume_token_owned = player.resume_token.clone();
            role_name = player.role.as_str().to_string();
        }
        let payload = WelcomePayload {
            player_id: player_id_owned,
            resume_token: resume_token_owned.clone(),
            room_id: room.id.clone(),
            seed: room.seed.clone(),
            role: role_name,
            room_state: room.state_name(),
            lobby: room.lobby_state(),
            cfg: NetConfig::default(),
            feature_flags: Default::default(),
        };
        let resume_token = resume_token_owned;
        let seq = room.next_seq();
        let lobby_state = room.lobby_state();
        let lobby_seq = room.next_seq();
        let lobby_frame = ServerFrame::LobbyState {
            meta: Envelope::boxed("lobby_state", lobby_seq, lobby_state),
        };
        Ok(AttachResult {
            welcome: ServerFrame::Welcome {
                meta: Envelope::boxed("welcome", seq, payload),
            },
            lobby: lobby_frame,
            sim: room.sim.clone(),
            resume_token,
        })
    }

    pub async fn detach_connection(&self, room_id: &str, player_id: &str) {
        if let Some(room_arc) = self.room(room_id).await {
            let mut room = room_arc.write().await;
            room.detach_queue(player_id);
        }
    }
}
