use axum::{routing::get, Router};
use std::net::SocketAddr;

mod config;
mod strategy;

async fn dashboard_handler() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("../dashboard.html"))
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let app = Router::new()
        .route("/", get(dashboard_handler));
        // TODO: Add routes for /api/trades, /api/portfolio

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    log::info!("Dashboard listening on http://{}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
