use crate::errors::{ErrorCode, ServerError};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const WS_TOKEN_TTL: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct TokenIssuer {
    secret: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WsTokenClaims {
    pub room_id: String,
    pub player_id: String,
    pub exp: usize,
    pub iat: usize,
}

impl WsTokenClaims {
    pub fn new(room_id: String, player_id: String) -> Self {
        let now = current_ts();
        Self {
            room_id,
            player_id,
            exp: (now + WS_TOKEN_TTL.as_secs()) as usize,
            iat: now as usize,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResumeToken(pub String);

impl TokenIssuer {
    pub fn new(secret: impl AsRef<[u8]>) -> Self {
        let key = secret.as_ref().to_vec();
        Self { secret: key }
    }

    pub fn mint_ws_token(&self, claims: WsTokenClaims) -> anyhow::Result<String> {
        let header = serde_json::json!({ "alg": "HS256", "typ": "JWT" });
        let header_bytes = serde_json::to_vec(&header)?;
        let payload_bytes = serde_json::to_vec(&claims)?;
        let header_enc = URL_SAFE_NO_PAD.encode(header_bytes);
        let payload_enc = URL_SAFE_NO_PAD.encode(payload_bytes);
        let signing_input = format!("{}.{}", header_enc, payload_enc);
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret).expect("HMAC");
        mac.update(signing_input.as_bytes());
        let signature = mac.finalize().into_bytes();
        let sig_enc = URL_SAFE_NO_PAD.encode(signature);
        Ok(format!("{}.{}", signing_input, sig_enc))
    }

    pub fn verify_ws_token(&self, token: &str) -> Result<WsTokenClaims, ServerError> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(ServerError::new(
                ErrorCode::Unauthorized,
                "invalid token format",
            ));
        }
        let signing_input = format!("{}.{}", parts[0], parts[1]);
        let signature = URL_SAFE_NO_PAD
            .decode(parts[2])
            .map_err(|_| ServerError::new(ErrorCode::Unauthorized, "invalid signature encoding"))?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret).expect("HMAC");
        mac.update(signing_input.as_bytes());
        mac.verify_slice(&signature)
            .map_err(|_| ServerError::new(ErrorCode::Unauthorized, "invalid signature"))?;
        let payload_bytes = URL_SAFE_NO_PAD
            .decode(parts[1])
            .map_err(|_| ServerError::new(ErrorCode::Unauthorized, "invalid payload encoding"))?;
        let claims: WsTokenClaims = serde_json::from_slice(&payload_bytes)
            .map_err(|_| ServerError::new(ErrorCode::Unauthorized, "invalid token payload"))?;
        if current_ts() > claims.exp as u64 {
            return Err(ServerError::new(ErrorCode::Unauthorized, "token expired"));
        }
        Ok(claims)
    }

    pub fn mint_resume_token(&self, room_id: &str, player_id: &str) -> ResumeToken {
        let mut buf = [0u8; 16];
        OsRng.fill_bytes(&mut buf);
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret).expect("HMAC");
        mac.update(room_id.as_bytes());
        mac.update(player_id.as_bytes());
        mac.update(&buf);
        let digest = mac.finalize().into_bytes();
        ResumeToken(URL_SAFE_NO_PAD.encode(digest))
    }
}

fn current_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_ws_token() {
        let issuer = TokenIssuer::new("secret");
        let claims = WsTokenClaims::new("room".into(), "player".into());
        let token = issuer.mint_ws_token(claims.clone()).unwrap();
        let decoded = issuer.verify_ws_token(&token).unwrap();
        assert_eq!(decoded.room_id, claims.room_id);
        assert_eq!(decoded.player_id, claims.player_id);
    }

    #[test]
    fn resume_token_is_randomized() {
        let issuer = TokenIssuer::new("secret");
        let a = issuer.mint_resume_token("room", "player");
        let b = issuer.mint_resume_token("room", "player");
        assert_ne!(a.0, b.0);
    }
}
