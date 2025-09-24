use crate::{
    auth::{TokenIssuer, WsTokenClaims},
    config::Config,
    errors::{ErrorCode, ServerError},
    protocol::{RoomSeed, WsBootstrap},
};
use dashmap::{DashMap, DashSet};
use rand::{distributions::Alphanumeric, Rng};
use serde::Serialize;
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
    pub players: DashSet<String>,
    pub resume_tokens: Arc<RwLock<HashMap<String, String>>>,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub regions: Vec<String>,
    pub server_pv: u32,
    pub rooms_active: usize,
    pub players_active: usize,
}

impl Matchmaker {
    pub fn new(config: Config, token_issuer: TokenIssuer) -> Self {
        Self {
            rooms: Arc::new(DashMap::new()),
            token_issuer,
            config,
        }
    }

    pub fn create_room(&self) -> anyhow::Result<WsBootstrap> {
        let room_id = self.generate_room_id();
        let seed = RoomSeed::random();
        let entry = Arc::new(RoomEntry {
            room_id: room_id.clone(),
            seed,
            capacity: self.config.room_capacity,
            players: DashSet::new(),
            resume_tokens: Arc::new(RwLock::new(HashMap::new())),
        });
        self.rooms.insert(room_id.clone(), entry.clone());

        let player_id = self.generate_player_id();
        entry.players.insert(player_id.clone());
        let ws_token = self
            .token_issuer
            .mint_ws_token(WsTokenClaims::new(room_id.clone(), player_id.clone()))?;
        Ok(WsBootstrap {
            room_id,
            seed,
            ws_url: self.config.ws_url.clone(),
            ws_token,
            player_id,
        })
    }

    pub fn join_room(&self, room_id: &str) -> Result<WsBootstrap, ServerError> {
        let entry = self
            .rooms
            .get(room_id)
            .ok_or_else(|| ServerError::new(ErrorCode::RoomNotFound, "room not found"))?;
        if entry.players.len() >= entry.capacity {
            return Err(ServerError::new(ErrorCode::RoomFull, "room full"));
        }
        let player_id = self.generate_player_id();
        entry.players.insert(player_id.clone());
        let ws_token = self
            .token_issuer
            .mint_ws_token(WsTokenClaims::new(room_id.to_string(), player_id.clone()))
            .map_err(|_| ServerError::new(ErrorCode::Unauthorized, "token error"))?;
        Ok(WsBootstrap {
            room_id: room_id.to_string(),
            seed: entry.seed,
            ws_url: self.config.ws_url.clone(),
            ws_token,
            player_id,
        })
    }

    pub fn leave_room(&self, room_id: &str, player_id: &str) {
        if let Some(entry) = self.rooms.get(room_id) {
            entry.players.remove(player_id);
            if entry.players.is_empty() {
                self.rooms.remove(room_id);
            }
        }
    }

    pub fn status(&self) -> StatusResponse {
        let rooms_active = self.rooms.len();
        let players_active = self.rooms.iter().map(|entry| entry.players.len()).sum();
        StatusResponse {
            regions: vec![self.config.region.clone()],
            server_pv: crate::protocol::SERVER_PV,
            rooms_active,
            players_active,
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
            let mut guard = entry.resume_tokens.write().await;
            guard.insert(player_id.to_string(), token);
        }
    }

    pub async fn validate_resume_token(
        &self,
        room_id: &str,
        player_id: &str,
        resume_token: &str,
    ) -> bool {
        if let Some(entry) = self.rooms.get(room_id) {
            let guard = entry.resume_tokens.read().await;
            if let Some(existing) = guard.get(player_id) {
                return existing == resume_token;
            }
        }
        false
    }
}
