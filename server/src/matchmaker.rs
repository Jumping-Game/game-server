use crate::{
    auth::{TokenIssuer, WsTokenClaims},
    config::Config,
    errors::{ErrorCode, ServerError},
    protocol::{RoomSeed, SERVER_PV},
};
use dashmap::{DashMap, DashSet};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct Matchmaker {
    rooms: Arc<DashMap<String, Arc<RoomEntry>>>,
    token_issuer: TokenIssuer,
    config: Config,
}

#[derive(Debug)]
pub struct RoomEntry {
    pub room_id: String,
    pub seed: RoomSeed,
    pub capacity: usize,
    pub players: DashMap<String, PlayerRecord>,
    pub names: DashSet<String>,
    pub resume_tokens: Arc<RwLock<HashMap<String, String>>>,
}

#[derive(Debug, Clone)]
pub struct PlayerRecord {
    pub name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub regions: Vec<RegionStatus>,
    pub server_pv: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionStatus {
    pub id: String,
    pub ping_ms: u32,
    pub ws_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoomRequest {
    pub name: String,
    pub region: String,
    pub max_players: usize,
    pub mode: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoomResponse {
    pub room_id: String,
    pub seed: RoomSeed,
    pub region: String,
    pub ws_url: String,
    pub ws_token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinRoomRequest {
    pub name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinRoomResponse {
    pub room_id: String,
    pub ws_url: String,
    pub ws_token: String,
}

impl Matchmaker {
    pub fn new(config: Config, token_issuer: TokenIssuer) -> Self {
        Self {
            rooms: Arc::new(DashMap::new()),
            token_issuer,
            config,
        }
    }

    pub fn create_room(
        &self,
        request: CreateRoomRequest,
    ) -> Result<CreateRoomResponse, ServerError> {
        if request.region != self.config.region {
            return Err(ServerError::new(
                ErrorCode::InvalidState,
                format!("unsupported region: {}", request.region),
            ));
        }

        if request.max_players == 0 {
            return Err(ServerError::new(
                ErrorCode::InvalidState,
                "maxPlayers must be greater than 0",
            ));
        }

        if request.max_players > self.config.room_capacity {
            return Err(ServerError::new(
                ErrorCode::InvalidState,
                "requested maxPlayers exceeds server capacity",
            ));
        }

        let room_id = self.generate_room_id();
        let seed = RoomSeed::random();
        let entry = Arc::new(RoomEntry {
            room_id: room_id.clone(),
            seed,
            capacity: request.max_players,
            players: DashMap::new(),
            names: DashSet::new(),
            resume_tokens: Arc::new(RwLock::new(HashMap::new())),
        });
        self.rooms.insert(room_id.clone(), entry.clone());

        let player_id = self.generate_player_id();
        entry.players.insert(
            player_id.clone(),
            PlayerRecord {
                name: request.name.clone(),
            },
        );
        entry.names.insert(request.name);
        let ws_token = match self
            .token_issuer
            .mint_ws_token(WsTokenClaims::new(room_id.clone(), player_id.clone()))
        {
            Ok(token) => token,
            Err(err) => {
                self.rooms.remove(&room_id);
                return Err(ServerError::with_source(
                    ErrorCode::Internal,
                    "failed to mint token",
                    err,
                ));
            }
        };
        Ok(CreateRoomResponse {
            room_id,
            seed,
            region: self.config.region.clone(),
            ws_url: self.config.ws_url.clone(),
            ws_token,
        })
    }

    pub fn join_room(
        &self,
        room_id: &str,
        request: JoinRoomRequest,
    ) -> Result<JoinRoomResponse, ServerError> {
        let entry = self
            .rooms
            .get(room_id)
            .ok_or_else(|| ServerError::new(ErrorCode::RoomNotFound, "room not found"))?;
        if entry.players.len() >= entry.capacity {
            return Err(ServerError::new(ErrorCode::RoomFull, "room full"));
        }
        if entry.names.contains(&request.name) {
            return Err(ServerError::new(
                ErrorCode::NameTaken,
                "display name already in use",
            ));
        }
        let player_id = self.generate_player_id();
        let player_name = request.name;
        entry.players.insert(
            player_id.clone(),
            PlayerRecord {
                name: player_name.clone(),
            },
        );
        entry.names.insert(player_name.clone());
        let ws_token = match self
            .token_issuer
            .mint_ws_token(WsTokenClaims::new(room_id.to_string(), player_id.clone()))
        {
            Ok(token) => token,
            Err(_) => {
                entry.players.remove(&player_id);
                entry.names.remove(&player_name);
                return Err(ServerError::new(ErrorCode::Unauthorized, "token error"));
            }
        };
        Ok(JoinRoomResponse {
            room_id: room_id.to_string(),
            ws_url: self.config.ws_url.clone(),
            ws_token,
        })
    }

    pub fn leave_room(&self, room_id: &str, player_id: &str) {
        if let Some(entry) = self.rooms.get(room_id) {
            if let Some(record) = entry.players.remove(player_id) {
                entry.names.remove(&record.1.name);
            }
            if entry.players.is_empty() {
                self.rooms.remove(room_id);
            }
        }
    }

    pub fn status(&self) -> StatusResponse {
        StatusResponse {
            regions: vec![RegionStatus {
                id: self.config.region.clone(),
                ping_ms: 0,
                ws_url: self.config.ws_url.clone(),
            }],
            server_pv: SERVER_PV,
        }
    }

    fn generate_room_id(&self) -> String {
        loop {
            let id: String = rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(6)
                .map(char::from)
                .collect();
            if !self.rooms.contains_key(&id) {
                break id;
            }
        }
    }

    fn generate_player_id(&self) -> String {
        let mut rng = rand::thread_rng();
        format!("p{}", rng.gen::<u64>())
    }

    pub async fn set_resume_token(&self, room_id: &str, player_id: &str, token: String) {
        if let Some(entry) = self.rooms.get(room_id) {
            if entry.players.contains_key(player_id) {
                let mut guard = entry.resume_tokens.write().await;
                guard.insert(player_id.to_string(), token);
            }
        }
    }

    pub async fn validate_resume_token(
        &self,
        room_id: &str,
        player_id: &str,
        resume_token: &str,
    ) -> bool {
        if let Some(entry) = self.rooms.get(room_id) {
            if !entry.players.contains_key(player_id) {
                return false;
            }
            let guard = entry.resume_tokens.read().await;
            if let Some(existing) = guard.get(player_id) {
                return existing == resume_token;
            }
        }
        false
    }

    pub fn room_seed(&self, room_id: &str) -> Option<RoomSeed> {
        self.rooms.get(room_id).map(|entry| entry.seed)
    }

    pub fn player_name(&self, room_id: &str, player_id: &str) -> Option<String> {
        self.rooms.get(room_id).and_then(|entry| {
            entry
                .players
                .get(player_id)
                .map(|record| record.name.clone())
        })
    }
}
