use chrono::Utc;
use rand::{distributions::Alphanumeric, Rng};

pub fn now_ms() -> u64 {
    Utc::now().timestamp_millis() as u64
}

pub fn generate_room_id() -> String {
    ulid::Ulid::new().to_string()
}

pub fn generate_player_id() -> String {
    ulid::Ulid::new().to_string()
}

pub fn generate_resume_token() -> String {
    let mut rng = rand::thread_rng();
    (0..32).map(|_| rng.sample(Alphanumeric) as char).collect()
}

pub fn clamp_countdown(sec: i64) -> u64 {
    sec.clamp(0, 5) as u64
}
