use figment::{providers::Env, Figment};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "default_api_port")]
    pub api_port: u16,
    #[serde(default = "default_ws_port")]
    pub ws_port: u16,
    #[serde(default = "default_bind")]
    pub api_bind: String,
    #[serde(default = "default_ws_bind")]
    pub ws_bind: String,
    #[serde(default = "default_jwt_secret")]
    pub jwt_secret: String,
    #[serde(default = "default_room_capacity")]
    pub default_max_players: u32,
    #[serde(default = "default_region")]
    pub region: String,
    #[serde(default = "default_enable_deflate")]
    pub enable_permessage_deflate: bool,
    #[serde(default = "default_ready_required")]
    pub require_ready: bool,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        let figment = Figment::new()
            .merge(figment::providers::Serialized::defaults(Config::default()))
            .merge(Env::prefixed("APP_"));
        Ok(figment.extract()?)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_port: default_api_port(),
            ws_port: default_ws_port(),
            api_bind: default_bind(),
            ws_bind: default_ws_bind(),
            jwt_secret: default_jwt_secret(),
            default_max_players: default_room_capacity(),
            region: default_region(),
            enable_permessage_deflate: default_enable_deflate(),
            require_ready: default_ready_required(),
            log_level: default_log_level(),
        }
    }
}

const fn default_api_port() -> u16 {
    8080
}

const fn default_ws_port() -> u16 {
    8081
}

fn default_bind() -> String {
    format!("0.0.0.0:{}", default_api_port())
}

fn default_ws_bind() -> String {
    format!("0.0.0.0:{}", default_ws_port())
}

fn default_jwt_secret() -> String {
    "dev-secret".to_string()
}

const fn default_room_capacity() -> u32 {
    4
}

fn default_region() -> String {
    "local-dev".to_string()
}

const fn default_enable_deflate() -> bool {
    true
}

const fn default_ready_required() -> bool {
    false
}

fn default_log_level() -> String {
    "info".to_string()
}
