#[derive(Debug, Clone, serde::Serialize)]
pub struct Candle {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Bubble {
    pub side: String, // "B" (Bullish) or "S" (Bearish)
    pub size: f64,
}

pub fn detect_absorption(candles: &[Candle], lookback: usize) -> Vec<Option<Bubble>> {
    if candles.len() < lookback { return vec![None; candles.len()]; }

    let volumes: Vec<f64> = candles.iter().map(|c| c.volume).collect();
    let k = 2.0 / (lookback as f64 + 1.0);
    
    let mut ema = vec![0.0; candles.len()];
    let mut stdev = vec![0.0; candles.len()];
    ema[0] = volumes[0];
    
    for i in 1..candles.len() {
        ema[i] = volumes[i] * k + ema[i-1] * (1.0 - k);
    }

    let base_threshold = 1.0;
    let mut out = vec![None; candles.len()];

    for i in lookback..candles.len() {
        let c = &candles[i];
        let range = c.high - c.low;
        if range == 0.0 { continue; }

        let start = i - lookback + 1;
        let sum: f64 = volumes[start..=i].iter().sum();
        let mean = sum / lookback as f64;
        
        let vs: f64 = volumes[start..=i].iter().map(|v| (v - mean).powi(2)).sum();
        stdev[i] = (vs / lookback as f64).sqrt();

        let upper_wick = c.high - c.open.max(c.close);
        let lower_wick = c.open.min(c.close) - c.low;
        let upper_wick_vol = c.volume * (upper_wick / range);
        let lower_wick_vol = c.volume * (lower_wick / range);

        let rvol = if ema[i] > 0.0 { c.volume / ema[i] } else { 0.0 };
        let z_score = if stdev[i] > 0.0 { c.volume / stdev[i] } else { 0.0 };
        let vol_score = (rvol * 0.6) + (z_score * 0.4);

        let mid_price = (c.high + c.low) / 2.0;
        let top_body = c.open.max(c.close);
        let low_body = c.open.min(c.close);

        let is_upper_wick = mid_price >= top_body && mid_price <= c.high;
        let is_lower_wick = mid_price <= low_body && mid_price >= c.low;

        let mut size = 0.0;
        if vol_score >= base_threshold + 6.0 { size = 4.0; }
        else if vol_score >= base_threshold + 3.0 { size = 3.0; }
        else if vol_score >= base_threshold + 2.0 { size = 2.0; }
        else if vol_score >= base_threshold + 1.0 { size = 1.5; }
        else if vol_score >= base_threshold { size = 1.0; }

        if size > 0.0 {
            if is_upper_wick && upper_wick_vol > 0.0 {
                out[i] = Some(Bubble { side: "S".into(), size });
            }
            if is_lower_wick && lower_wick_vol > 0.0 {
                out[i] = Some(Bubble { side: "B".into(), size });
            }
        }
    }
    out
}
