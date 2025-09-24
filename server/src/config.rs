use serde::Deserialize;
use std::{env, fs, path::Path};

const DEFAULT_CONFIG_PATH: &str = "configs/server.example.toml";

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_bind_address")]
    pub bind_address: String,
    #[serde(default = "default_ws_url")]
    pub ws_url: String,
    #[serde(default = "default_region")]
    pub region: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_room_capacity")]
    pub room_capacity: usize,
    #[serde(default = "default_snapshot_rate")]
    pub snapshot_rate_hz: u32,
    #[serde(default = "default_tick_rate")]
    pub tick_rate_hz: u32,
    #[serde(default = "default_max_rollback")]
    pub max_rollback_ticks: u32,
    #[serde(default = "default_input_lead")]
    pub input_lead_ticks: u32,
    #[serde(default = "default_token_secret")]
    pub token_secret: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bind_address: default_bind_address(),
            ws_url: default_ws_url(),
            region: default_region(),
            log_level: default_log_level(),
            room_capacity: default_room_capacity(),
            snapshot_rate_hz: default_snapshot_rate(),
            tick_rate_hz: default_tick_rate(),
            max_rollback_ticks: default_max_rollback(),
            input_lead_ticks: default_input_lead(),
            token_secret: default_token_secret(),
        }
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let path = env::var("SERVER_CONFIG").unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string());
        if Path::new(&path).exists() {
            let s = fs::read_to_string(&path)?;
            let mut cfg: Config = toml::from_str(&s)?;
            cfg.apply_env_overrides();
            Ok(cfg)
        } else {
            let mut cfg = Config::default();
            cfg.apply_env_overrides();
            Ok(cfg)
        }
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(val) = env::var("SERVER_BIND_ADDRESS") {
            self.bind_address = val;
        }
        if let Ok(val) = env::var("SERVER_WS_URL") {
            self.ws_url = val;
        }
        if let Ok(val) = env::var("SERVER_REGION") {
            self.region = val;
        }
        if let Ok(val) = env::var("SERVER_LOG_LEVEL") {
            self.log_level = val;
        }
        if let Ok(val) = env::var("SERVER_ROOM_CAPACITY") {
            if let Ok(parsed) = val.parse() {
                self.room_capacity = parsed;
            }
        }
        if let Ok(val) = env::var("SERVER_SNAPSHOT_RATE") {
            if let Ok(parsed) = val.parse() {
                self.snapshot_rate_hz = parsed;
            }
        }
        if let Ok(val) = env::var("SERVER_TICK_RATE") {
            if let Ok(parsed) = val.parse() {
                self.tick_rate_hz = parsed;
            }
        }
        if let Ok(val) = env::var("SERVER_MAX_ROLLBACK") {
            if let Ok(parsed) = val.parse() {
                self.max_rollback_ticks = parsed;
            }
        }
        if let Ok(val) = env::var("SERVER_INPUT_LEAD") {
            if let Ok(parsed) = val.parse() {
                self.input_lead_ticks = parsed;
            }
        }
        if let Ok(val) = env::var("SERVER_TOKEN_SECRET") {
            self.token_secret = val;
        }
    }
}

fn default_bind_address() -> String {
    "0.0.0.0:3000".to_string()
}
fn default_ws_url() -> String {
    "wss://localhost:3000/v1/ws".to_string()
}
fn default_region() -> String {
    "local".to_string()
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_room_capacity() -> usize {
    8
}
fn default_snapshot_rate() -> u32 {
    10
}
fn default_tick_rate() -> u32 {
    60
}
fn default_max_rollback() -> u32 {
    120
}
fn default_input_lead() -> u32 {
    2
}
fn default_token_secret() -> String {
    "development-secret-change-me".to_string()
}
