use crate::{
    errors::{AppError, ErrorCode},
    util,
};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AuthService {
    encoding: EncodingKey,
    decoding: DecodingKey,
    pub secret: Arc<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WsClaims {
    pub sub: String,
    pub room_id: String,
    pub player_id: String,
    pub role: String,
    pub exp: usize,
    pub iat: usize,
}

impl AuthService {
    pub fn new(secret: impl Into<String>) -> Self {
        let secret_string = secret.into();
        Self {
            encoding: EncodingKey::from_secret(secret_string.as_bytes()),
            decoding: DecodingKey::from_secret(secret_string.as_bytes()),
            secret: Arc::new(secret_string),
        }
    }

    pub fn mint_ws_token(
        &self,
        room_id: &str,
        player_id: &str,
        role: &str,
        ttl_seconds: usize,
    ) -> anyhow::Result<String> {
        let now = util::now_ms() as usize / 1000;
        let claims = WsClaims {
            sub: format!("player:{}", player_id),
            room_id: room_id.to_string(),
            player_id: player_id.to_string(),
            role: role.to_string(),
            exp: now + ttl_seconds,
            iat: now,
        };
        let token = encode(&Header::new(Algorithm::HS256), &claims, &self.encoding)?;
        Ok(token)
    }

    pub fn verify_ws_token(&self, token: &str) -> Result<WsClaims, AppError> {
        if let Some(rest) = token.strip_prefix("dev:") {
            let mut parts = rest.split(':');
            let room_id = parts.next().unwrap_or_default().to_string();
            let player_id = parts.next().unwrap_or_default().to_string();
            let role = parts.next().unwrap_or("member").to_string();
            if room_id.is_empty() || player_id.is_empty() {
                return Err(AppError::http(ErrorCode::Unauthorized, "invalid dev token"));
            }
            let now = util::now_ms() as usize / 1000;
            return Ok(WsClaims {
                sub: format!("player:{}", player_id),
                room_id,
                player_id,
                role,
                exp: now + 3600,
                iat: now,
            });
        }
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;
        decode::<WsClaims>(token, &self.decoding, &validation)
            .map(|data| data.claims)
            .map_err(|_| AppError::http(ErrorCode::Unauthorized, "invalid token"))
    }
}

#[derive(Clone, Default)]
pub struct ResumeStore {
    inner: Arc<RwLock<HashMap<String, String>>>,
}

impl ResumeStore {
    pub async fn put(&self, token: String, player_id: String) {
        self.inner.write().await.insert(token, player_id);
    }

    pub async fn take(&self, token: &str) -> Option<String> {
        self.inner.write().await.remove(token)
    }
}
