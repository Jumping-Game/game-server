use axum::{http::StatusCode, response::IntoResponse};
use once_cell::sync::{Lazy, OnceCell};
use prometheus::{Encoder, IntCounter, Registry, TextEncoder};

static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);
static INIT: OnceCell<()> = OnceCell::new();

pub static ROOMS_ACTIVE: Lazy<IntCounter> =
    Lazy::new(|| IntCounter::new("rooms_active_total", "rooms currently active").unwrap());

pub fn init() {
    INIT.get_or_init(|| {
        let _ = REGISTRY.register(Box::new(ROOMS_ACTIVE.clone()));
    });
}

pub async fn metrics_handler() -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = Vec::new();
    if encoder.encode(&metric_families, &mut buffer).is_ok() {
        let headers = [(axum::http::header::CONTENT_TYPE, encoder.format_type())];
        (StatusCode::OK, headers, buffer).into_response()
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, "metrics encoding error").into_response()
    }
}
