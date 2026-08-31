use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kline {
    pub open_time: u64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub close_time: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderRequest {
    pub symbol: String,
    pub side: String,          // "BUY" | "SELL"
    #[serde(rename = "type")]
    pub order_type: String,    // "LIMIT" | "MARKET"
    pub quantity: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<f64>,
    pub time_in_force: Option<String>,
    pub reduce_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    pub asset: String,
    pub balance: f64,
    pub available_balance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionRisk {
    pub symbol: String,
    pub position_amt: f64,
    pub entry_price: f64,
    pub unrealized_profit: f64,
    pub leverage: f64,
}
