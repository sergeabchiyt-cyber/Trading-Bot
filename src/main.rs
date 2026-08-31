use axum::{routing::get, Router};
use std::net::SocketAddr;
use std::sync::Arc;

mod config;
mod exchange;
mod risk;
mod strategy;

async fn dashboard_handler() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("../dashboard.html"))
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let cfg = Arc::new(config::Config::from_env());
    let client = exchange::binance::BinanceClient::new_testnet(
        cfg.binance_testnet_api_key.clone(),
        cfg.binance_testnet_secret.clone(),
    );

    let engine_cfg = cfg.clone();
    tokio::spawn(async move {
        let mut engine = strategy::engine::Engine::new(engine_cfg, client);
        if let Err(e) = engine.run().await {
            log::error!("engine died: {e}");
        }
    });

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let app = Router::new()
        .route("/", get(dashboard_handler))
        .route("/healthz", get(|| async { "ok" }));
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    log::info!("listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
