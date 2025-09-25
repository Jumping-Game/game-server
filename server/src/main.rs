use server::{config::Config, http, ws::WsServer};

use anyhow::Context;
use axum::Router;
use std::sync::Arc;
use tokio::signal;
use tracing_subscriber::{fmt, layer::SubscriberExt, EnvFilter, Registry};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load().context("load config")?;
    init_tracing(&config);

    let state = Arc::new(http::HttpState::new(config.clone()));
    let router: Router = http::router(state.clone());
    let addr = config.api_bind.parse().context("invalid api bind")?;

    let http_server = axum::Server::bind(&addr).serve(router.into_make_service());
    let ws_server = WsServer::new(state.clone()).run();

    tokio::select! {
        res = http_server => {
            res.context("http server error")?;
        }
        res = ws_server => {
            res.context("ws server error")?;
        }
        _ = shutdown_signal() => {
            tracing::info!("shutdown");
        }
    }

    Ok(())
}

fn init_tracing(config: &Config) {
    let filter = config
        .log_level
        .parse::<EnvFilter>()
        .unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = fmt::layer().with_target(false).json().flatten_event(true);
    let subscriber = Registry::default().with(filter).with(fmt_layer);
    tracing::subscriber::set_global_default(subscriber).expect("set global subscriber");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
