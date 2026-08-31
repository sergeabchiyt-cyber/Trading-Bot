use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub opened_at: u64,
    pub closed_at: Option<u64>,
    pub symbol: String,
    pub side: String,      // "BUY" | "SELL"
    pub level: String,     // "PoC" | "VAH" | "VAL"
    pub entry: f64,
    pub initial_sl: f64,   // frozen at entry — pre-BE, pre-trail
    pub tp: f64,
    pub qty: f64,
    pub rr: f64,
    pub high_prob: bool,
    pub adx: f64,
    pub bubble_size: f64,
    pub status: String,    // OPEN | TP | SL | TRAIL
    pub exit_price: Option<f64>,
    pub pnl_r: Option<f64>,
}

pub struct TradeLog {
    path: PathBuf,
    pub trades: Vec<TradeRecord>,
}

impl TradeLog {
    pub fn load<P: AsRef<Path>>(path: P) -> Self {
        let path = path.as_ref().to_path_buf();
        let trades = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, trades }
    }

    pub fn record_open(&mut self, rec: TradeRecord) -> usize {
        self.trades.push(rec);
        self.save();
        self.trades.len() - 1
    }

    pub fn record_close(&mut self, idx: usize, exit_price: f64, status: &str) {
        if let Some(t) = self.trades.get_mut(idx) {
            let risk = (t.entry - t.initial_sl).abs();
            let dir = if t.side == "BUY" { 1.0 } else { -1.0 };
            t.exit_price = Some(exit_price);
            t.closed_at = Some(now_s());
            t.status = status.to_string();
            t.pnl_r = if risk > 0.0 {
                Some(dir * (exit_price - t.entry) / risk)
            } else {
                None
            };
            self.save();
        }
    }

    fn save(&self) {
        if let Ok(json) = serde_json::to_vec_pretty(&self.trades) {
            let _ = fs::write(&self.path, json);
        }
    }
}

fn now_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
