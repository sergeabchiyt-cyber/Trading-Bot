pub struct Config {
    pub binance_testnet_api_key: String,
    pub binance_testnet_secret: String,
    pub risk_pct: f64,       // 0.02
    pub max_leverage: f64,   // 15.0
    pub trail_activation_r: f64, // 1.2
    pub trail_pullback_pct: f64, // 0.012
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            binance_testnet_api_key: std::env::var("BINANCE_TESTNET_KEY").unwrap_or_default(),
            binance_testnet_secret: std::env::var("BINANCE_TESTNET_SECRET").unwrap_or_default(),
            risk_pct: 0.02,
            max_leverage: 15.0,
            trail_activation_r: 1.2,
            trail_pullback_pct: 0.012,
        }
    }
}
