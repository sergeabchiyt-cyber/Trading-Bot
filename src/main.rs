use axum::{extract::State, routing::get, Json, Router};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

mod config;
mod exchange;
mod risk;
mod strategy;
mod tradelog;

#[derive(Clone)]
struct AppState {
    trades: Arc<RwLock<Vec<tradelog::TradeRecord>>>,
    client: Arc<exchange::binance::BinanceClient>,
}

async fn dashboard_handler() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("../dashboard.html"))
}

async fn trades_handler(State(s): State<AppState>) -> Json<Vec<tradelog::TradeRecord>> {
    Json(s.trades.read().await.clone())
}

async fn portfolio_handler(State(s): State<AppState>) -> Json<Value> {
    let trades = s.trades.read().await.clone();
    let closed: Vec<_> = trades.iter().filter(|t| t.closed_at.is_some()).collect();
    let wins = closed.iter().filter(|t| t.pnl_r.unwrap_or(0.0) > 0.0).count();
    let win_rate = if closed.is_empty() {
        0.0
    } else {
        wins as f64 / closed.len() as f64 * 100.0
    };
    let equity = s
        .client
        .account_balance()
        .await
        .ok()
        .and_then(|b| {
            b.as_array().and_then(|arr| {
                arr.iter()
                    .find(|i| i["asset"].as_str() == Some("USDT"))
                    .and_then(|i| i["balance"].as_str())
                    .and_then(|v| v.parse::<f64>().ok())
            })
        })
        .unwrap_or(0.0);
    let open = trades
        .iter()
        .find(|t| t.closed_at.is_none())
        .map(|t| {
            json!({
                "side": t.side,
                "entry": t.entry,
                "sl": t.initial_sl,
                "tp": t.tp,
                "level": t.level,
            })
        });
    Json(json!({
        "equity": equity,
        "openPosition": open,
        "totalTrades": trades.len(),
        "closedTrades": closed.len(),
        "winRate": win_rate,
        "lastPnlR": closed.last().and_then(|t| t.pnl_r),
    }))
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let cfg = Arc::new(config::Config::from_env());
    let client = Arc::new(exchange::binance::BinanceClient::new_testnet(
        cfg.binance_testnet_api_key.clone(),
        cfg.binance_testnet_secret.clone(),
    ));
    let shared_trades = Arc::new(RwLock::new(Vec::<tradelog::TradeRecord>::new()));

    let engine_cfg = cfg.clone();
    let engine_client = client.clone();
    let engine_trades = shared_trades.clone();
    tokio::spawn(async move {
        let mut engine = strategy::engine::Engine::new(engine_cfg, engine_client, engine_trades);
        if let Err(e) = engine.run().await {
            log::error!("engine died: {e}");
        }
    });

    let state = AppState {
        trades: shared_trades,
        client,
    };
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let app = Router::new()
        .route("/", get(dashboard_handler))
        .route("/healthz", get(|| async { "ok" }))
        .route("/api/trades", get(trades_handler))
        .route("/api/portfolio", get(portfolio_handler))
        .with_state(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    log::info!("listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
