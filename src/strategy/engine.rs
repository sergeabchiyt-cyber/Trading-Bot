use anyhow::Result;
use chrono::{Datelike, TimeZone, Utc};
use log::{info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::Config;
use crate::exchange::binance::BinanceClient;
use crate::exchange::types::OrderRequest;
use crate::risk::manager::PositionManager;
use crate::tradelog::{TradeLog, TradeRecord};

use super::indicators::*;
use super::orderflow::{detect_absorption, Candle};

const SYMBOL: &str = "BTCUSDT";
const TAP_TOLERANCE: f64 = 0.001;
const MIN_BUBBLE_SIZE: f64 = 1.5;
const ATR_SL_MULT: f64 = 0.5;
const TRADES_FILE: &str = "trades.json";

#[derive(Debug, Clone)]
struct TapRecord {
    taps: u32,
    confirmed_4h: bool,
}

#[derive(Debug, Clone)]
struct EntryMeta {
    level_name: String,
    rr: f64,
    high_prob: bool,
    adx: f64,
    bubble_size: f64,
}

enum PosState {
    Flat,
    PendingEntry {
        side: String,
        order_id: u64,
        entry: f64,
        sl: f64,
        tp: f64,
        qty: f64,
        meta: EntryMeta,
    },
    Open {
        pm: PositionManager,
        stop_order_id: u64,
    },
}

pub struct Engine {
    cfg: Arc<Config>,
    client: Arc<BinanceClient>,
    state: PosState,
    taps: HashMap<u64, TapRecord>,
    last_eval_ts: u64,
    trades: TradeLog,
    open_idx: Option<usize>,
    shared_trades: Arc<RwLock<Vec<TradeRecord>>>,
}

impl Engine {
    pub fn new(
        cfg: Arc<Config>,
        client: Arc<BinanceClient>,
        shared_trades: Arc<RwLock<Vec<TradeRecord>>>,
    ) -> Self {
        Self {
            cfg,
            client,
            state: PosState::Flat,
            taps: HashMap::new(),
            last_eval_ts: 0,
            trades: TradeLog::load(TRADES_FILE),
            open_idx: None,
            shared_trades,
        }
    }

    async fn sync_trades(&self) {
        *self.shared_trades.write().await = self.trades.trades.clone();
    }

    pub async fn run(&mut self) -> Result<()> {
        self.sync_trades().await;
        self.client
            .set_leverage(SYMBOL, self.cfg.max_leverage as u32)
            .await?;
        self.rehydrate().await?;

        loop {
            let now = Utc::now().timestamp() as u64;
            let candle_boundary = now - (now % 900);

            match &self.state {
                PosState::Flat => {
                    if candle_boundary > self.last_eval_ts {
                        self.last_eval_ts = candle_boundary;
                        if let Err(e) = self.evaluate().await {
                            warn!("evaluate: {e}");
                        }
                    }
                }
                PosState::PendingEntry { .. } => {
                    if let Err(e) = self.check_fill().await {
                        warn!("fill-check: {e}");
                    }
                }
                PosState::Open { .. } => {
                    if let Err(e) = self.manage().await {
                        warn!("manage: {e}");
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }

    /// On boot: reconcile with exchange — never trust local memory after a Render restart.
    /// Note: rehydrated positions have no trade-log entry (open_idx = None).
    async fn rehydrate(&mut self) -> Result<()> {
        let pos = self.client.position_risk(SYMBOL).await?;
        let (amt, entry): (f64, f64) = pos
            .as_array()
            .and_then(|a| a.first())
            .map(|p| {
                (
                    p["positionAmt"].as_str().unwrap_or("0").parse().unwrap_or(0.0),
                    p["entryPrice"].as_str().unwrap_or("0").parse().unwrap_or(0.0),
                )
            })
            .unwrap_or((0.0, 0.0));

        if amt.abs() > 0.0 {
            info!("rehydrated open position: {amt} @ {entry}");
            let is_long = amt > 0.0;
            let mut pm = PositionManager::new(
                &self.cfg,
                0.0,
                entry,
                entry * if is_long { 0.995 } else { 1.005 },
                entry * if is_long { 1.02 } else { 0.98 },
                is_long,
            );
            pm.qty = amt.abs();
            self.state = PosState::Open {
                pm,
                stop_order_id: 0,
            };
        }
        Ok(())
    }

    async fn evaluate(&mut self) -> Result<()> {
        // ---- data ----
        let k15 = self.client.fetch_klines(SYMBOL, "15m", 400).await?;
        let mut c15 = parse_klines(&k15);
        c15.pop();

        let k1h = self.client.fetch_klines(SYMBOL, "1h", 200).await?;
        let c1h = parse_klines(&k1h);
        let weekly: Vec<Candle> = c1h
            .into_iter()
            .filter(|c| c.open_time >= week_start_ms())
            .collect();

        let k4h = self.client.fetch_klines(SYMBOL, "4h", 12).await?;
        let c4h = parse_klines(&k4h);

        if c15.len() < 150 || weekly.len() < 24 {
            return Ok(());
        }

        // ---- checkmark 1: weekly VP levels + bias ----
        let vp = match calc_volume_profile(&weekly, 25.0) {
            Some(v) => v,
            None => return Ok(()),
        };
        let price = c15.last().unwrap().close;

        let candidates: Vec<(f64, bool, &str)> = if price > vp.poc {
            vec![(vp.poc, true, "PoC"), (vp.vah, false, "VAH")]
        } else {
            vec![(vp.poc, false, "PoC"), (vp.val, true, "VAL")]
        };

        let mut tapped: Vec<(f64, bool, String, bool)> = Vec::new();
        for (level, is_long, name) in candidates {
            if (price - level).abs() / level <= TAP_TOLERANCE {
                let confirmed_4h = confirm_4h(&c4h, level, is_long);
                let key = level.round() as u64;
                let rec = self.taps.entry(key).or_insert(TapRecord {
                    taps: 0,
                    confirmed_4h: false,
                });
                rec.taps += 1;
                if confirmed_4h {
                    rec.confirmed_4h = true;
                }
                let hp = rec.taps > 1 && rec.confirmed_4h;
                info!("TAP {name} @ {level:.1} | 4h:{confirmed_4h} | high_prob:{hp}");
                tapped.push((level, is_long, name.to_string(), hp));
            }
        }
        if tapped.is_empty() {
            return Ok(());
        }
        let (level, is_long, lvl_name, high_prob) = tapped.remove(0);

        // ---- checkmark 2: ADX 15m rising > 20 + DI alignment ----
        let bars = calc_adx(&c15, 14);
        let cur = match bars.last() {
            Some(b) => b,
            None => return Ok(()),
        };
        let prev_adx = match bars
            .get(bars.len().saturating_sub(2))
            .and_then(|b| b.adx)
        {
            Some(a) => a,
            None => return Ok(()),
        };
        let cur_adx = match cur.adx {
            Some(a) => a,
            None => return Ok(()),
        };
        if !(cur_adx > 20.0 && cur_adx > prev_adx) {
            return Ok(());
        }
        let (pdi, mdi) = match (cur.plus_di, cur.minus_di) {
            (Some(a), Some(b)) => (a, b),
            _ => return Ok(()),
        };
        let di_ok = if is_long { pdi > mdi } else { mdi > pdi };
        if !di_ok {
            return Ok(());
        }

        // ---- checkmark 3: absorption bubble in trade direction ----
        let bubbles = detect_absorption(&c15, 100);
        let want = if is_long { "B" } else { "S" };
        let (confirmed, bubble_size) = match bubbles.last().and_then(|b| b.as_ref()) {
            Some(b) if b.side == want => (b.size >= MIN_BUBBLE_SIZE, b.size),
            _ => (false, 0.0),
        };
        if !confirmed {
            return Ok(());
        }

        // ---- risk geometry ----
        let atr = calc_atr(&c15, 14)
            .last()
            .copied()
            .flatten()
            .unwrap_or(price * 0.001);
        let sl = if is_long {
            level - ATR_SL_MULT * atr
        } else {
            level + ATR_SL_MULT * atr
        };

        let (swing_h, swing_l) = match find_3d_swing(&c15) {
            Some(s) => s,
            None => return Ok(()),
        };
        let (f1, f2) = fib_golden_pocket(swing_h, swing_l, is_long);
        let tp = if is_long { f2 } else { f1 };
        let risk = (level - sl).abs();
        let reward = (tp - level).abs();
        if risk <= 0.0 || reward / risk < 2.0 {
            info!("FIB GP rejected: RR {} < 2.0", reward / risk);
            return Ok(());
        }
        let rr = reward / risk;

        // ---- sizing + entry ----
        let equity = self.equity().await?;
        let qty = round_qty((equity * self.cfg.risk_pct) / risk);
        if qty <= 0.0 {
            warn!("qty rounds to 0 — risk too small for BTCUSDT stepSize");
            return Ok(());
        }
        let side = if is_long { "BUY" } else { "SELL" };

        let resp = self
            .client
            .place_order(&OrderRequest {
                symbol: SYMBOL.into(),
                side: side.into(),
                order_type: "LIMIT".into(),
                quantity: qty,
                price: Some(level),
                stop_price: None,
                time_in_force: Some("GTC".into()),
                reduce_only: None,
            })
            .await?;
        let order_id = resp["orderId"].as_u64().unwrap_or(0);

        info!("ENTRY {side} {lvl_name} @ {level:.1} qty={qty} SL={sl:.1} TP={tp:.1} RR={rr:.2}");
        self.state = PosState::PendingEntry {
            side: side.into(),
            order_id,
            entry: level,
            sl,
            tp,
            qty,
            meta: EntryMeta {
                level_name: lvl_name,
                rr,
                high_prob,
                adx: cur_adx,
                bubble_size,
            },
        };
        Ok(())
    }

    async fn check_fill(&mut self) -> Result<()> {
        let (order_id, side, entry, sl, tp, qty, meta) = match &self.state {
            PosState::PendingEntry {
                order_id,
                side,
                entry,
                sl,
                tp,
                qty,
                meta,
            } => (*order_id, side.clone(), *entry, *sl, *tp, *qty, meta.clone()),
            _ => return Ok(()),
        };
        let status = self.client.order_status(SYMBOL, order_id).await?;
        let st = status["status"].as_str().unwrap_or("");

        if st == "FILLED" {
            let stop_side = if side == "BUY" { "SELL" } else { "BUY" };

            let resp = self
                .client
                .place_order(&OrderRequest {
                    symbol: SYMBOL.into(),
                    side: stop_side.into(),
                    order_type: "STOP_MARKET".into(),
                    quantity: qty,
                    price: None,
                    stop_price: Some(sl),
                    time_in_force: None,
                    reduce_only: Some(true),
                })
                .await?;
            let stop_id = resp["orderId"].as_u64().unwrap_or(0);

            if let Err(e) = self
                .client
                .place_order(&OrderRequest {
                    symbol: SYMBOL.into(),
                    side: stop_side.into(),
                    order_type: "TAKE_PROFIT_MARKET".into(),
                    quantity: qty,
                    price: None,
                    stop_price: Some(tp),
                    time_in_force: None,
                    reduce_only: Some(true),
                })
                .await
            {
                warn!("TP order failed (local fallback covers): {e}");
            }

            let rec = TradeRecord {
                opened_at: Utc::now().timestamp() as u64,
                closed_at: None,
                symbol: SYMBOL.into(),
                side: side.clone(),
                level: meta.level_name,
                entry,
                initial_sl: sl,
                tp,
                qty,
                rr: meta.rr,
                high_prob: meta.high_prob,
                adx: meta.adx,
                bubble_size: meta.bubble_size,
                status: "OPEN".into(),
                exit_price: None,
                pnl_r: None,
            };
            let idx = self.trades.record_open(rec);
            self.open_idx = Some(idx);
            self.sync_trades().await;

            let pm = PositionManager::new(&self.cfg, 0.0, entry, sl, tp, side == "BUY");
            self.state = PosState::Open {
                pm,
                stop_order_id: stop_id,
            };
            info!("FILLED — exchange SL #{stop_id} live @ {sl:.1}");
        } else if st == "CANCELED" || st == "EXPIRED" {
            self.state = PosState::Flat;
        }
        Ok(())
    }

    async fn manage(&mut self) -> Result<()> {
        let price = self.client.mark_price(SYMBOL).await?;
        let pos = self.client.position_risk(SYMBOL).await?;
        let amt: f64 = pos
            .as_array()
            .and_then(|a| a.first())
            .and_then(|p| p["positionAmt"].as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);

        // exchange stop/TP already fired (possibly while instance slept)
        if amt.abs() == 0.0 {
            if let PosState::Open { pm, .. } = &self.state {
                let status = classify_exit(pm, price);
                if let Some(idx) = self.open_idx.take() {
                    self.trades.record_close(idx, price, status);
                    self.sync_trades().await;
                }
                info!("position closed by exchange orders ({status}) — going flat");
            }
            self.state = PosState::Flat;
            return Ok(());
        }

        let exit_now = match &self.state {
            PosState::Open { pm, .. } => pm.should_exit(price),
            _ => return Ok(()),
        };

        if exit_now {
            let (qty, is_long, status) = match &self.state {
                PosState::Open { pm, .. } => (pm.qty, pm.is_long, classify_exit(pm, price)),
                _ => return Ok(()),
            };
            self.client
                .place_order(&OrderRequest {
                    symbol: SYMBOL.into(),
                    side: if is_long { "SELL" } else { "BUY" }.into(),
                    order_type: "MARKET".into(),
                    quantity: qty,
                    price: None,
                    stop_price: None,
                    time_in_force: None,
                    reduce_only: Some(true),
                })
                .await
                .ok();
            if let Some(idx) = self.open_idx.take() {
                self.trades.record_close(idx, price, status);
                self.sync_trades().await;
            }
            info!("EXIT signal @ ~{price:.1} ({status}) — flattened");
            self.state = PosState::Flat;
            return Ok(());
        }

        // amend SL only when it moved meaningfully (> 0.05%) — no amend spam
        let (old_sl, new_sl, stop_id, is_long, qty) = match &mut self.state {
            PosState::Open { pm, stop_order_id } => {
                let old = pm.initial_sl;
                let new = pm.update_sl(&self.cfg, price);
                (old, new, *stop_order_id, pm.is_long, pm.qty)
            }
            _ => return Ok(()),
        };

        if new_sl > 0.0 && old_sl > 0.0 && (new_sl - old_sl).abs() / old_sl > 0.0005 {
            self.client.cancel_order(SYMBOL, stop_id).await.ok();
            let stop_side = if is_long { "SELL" } else { "BUY" };
            let resp = self
                .client
                .place_order(&OrderRequest {
                    symbol: SYMBOL.into(),
                    side: stop_side.into(),
                    order_type: "STOP_MARKET".into(),
                    quantity: qty,
                    price: None,
                    stop_price: Some(new_sl),
                    time_in_force: None,
                    reduce_only: Some(true),
                })
                .await?;
            if let PosState::Open { stop_order_id, .. } = &mut self.state {
                *stop_order_id = resp["orderId"].as_u64().unwrap_or(0);
            }
            info!("SL amended -> {new_sl:.1}");
        }
        Ok(())
    }

    async fn equity(&self) -> Result<f64> {
        let bal = self.client.account_balance().await?;
        for item in bal.as_array().unwrap_or(&vec![]) {
            if item["asset"].as_str() == Some("USDT") {
                return Ok(item["balance"]
                    .as_str()
                    .unwrap_or("0")
                    .parse()
                    .unwrap_or(0.0));
            }
        }
        Ok(0.0)
    }
}

fn classify_exit(pm: &PositionManager, exit_price: f64) -> &'static str {
    if pm.trailing_active {
        return "TRAIL";
    }
    if pm.is_long {
        if exit_price >= pm.tp * 0.999 { "TP" } else { "SL" }
    } else if exit_price <= pm.tp * 1.001 {
        "TP"
    } else {
        "SL"
    }
}

// ---- helpers ----
fn parse_klines(raw: &[serde_json::Value]) -> Vec<Candle> {
    raw.iter()
        .filter_map(|k| {
            let arr = k.as_array()?;
            Some(Candle {
                open_time: arr[0].as_u64()?,
                open: arr[1].as_str()?.parse().ok()?,
                high: arr[2].as_str()?.parse().ok()?,
                low: arr[3].as_str()?.parse().ok()?,
                close: arr[4].as_str()?.parse().ok()?,
                volume: arr[5].as_str()?.parse().ok()?,
                close_time: arr[6].as_u64()?,
            })
        })
        .collect()
}

fn week_start_ms() -> u64 {
    let now = Utc::now();
    let dow = now.weekday().num_days_from_sunday();
    let sunday = now.date_naive() - chrono::Duration::days(dow as i64);
    Utc.from_utc_datetime(&sunday.and_hms_opt(0, 0, 0).unwrap())
        .timestamp_millis() as u64
}

fn confirm_4h(c4h: &[Candle], level: f64, is_long: bool) -> bool {
    match c4h.last() {
        None => false,
        Some(c) => {
            let touched = c.low <= level && c.high >= level;
            touched && if is_long { c.close > level } else { c.close < level }
        }
    }
}

fn round_qty(q: f64) -> f64 {
    (q * 1000.0).floor() / 1000.0
}
