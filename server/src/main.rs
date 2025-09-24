mod auth;
mod backpressure;
mod config;
mod errors;
mod http;
mod matchmaker;
mod metrics;
mod presence;
mod protocol;
mod rate_limit;
mod room;
pub mod sim;
mod ws;

use anyhow::Context;
use axum::Router;
use tokio::signal;
use tracing_subscriber::{fmt, layer::SubscriberExt, EnvFilter, Registry};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = config::Config::load().context("load config")?;
    init_tracing(&config);

    let state = http::HttpState::new(config.clone());
    let router: Router = http::router(state.clone());

    let addr = config
        .bind_address
        .parse()
        .context("invalid bind address")?;
    tracing::info!("starting_http {}", addr);
    let server = axum::Server::bind(&addr).serve(router.into_make_service());

    tokio::select! {
        res = server => {
            res.context("server error")?;
        }
        _ = shutdown_signal() => {
            tracing::info!("shutdown_signal");
        }
    }

    Ok(())
}

fn init_tracing(config: &config::Config) {
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
