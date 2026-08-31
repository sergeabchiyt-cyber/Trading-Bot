use super::orderflow::Candle;

#[derive(Debug, Clone)]
pub struct AdxBar {
    pub adx: Option<f64>,
    pub plus_di: Option<f64>,
    pub minus_di: Option<f64>,
}

/// Wilder's smoothing ADX + directional indicators
pub fn calc_adx(candles: &[Candle], period: usize) -> Vec<AdxBar> {
    let n = candles.len();
    let mut out: Vec<AdxBar> = vec![AdxBar { adx: None, plus_di: None, minus_di: None }; n];
    if n < period * 2 + 1 {
        return out;
    }

    let mut tr_s = vec![0.0; n];
    let mut plus_dm_s = vec![0.0; n];
    let mut minus_dm_s = vec![0.0; n];
    let mut dx = vec![0.0; n];

    for i in 1..n {
        let h = candles[i].high;
        let l = candles[i].low;
        let pc = candles[i - 1].close;
        tr_s[i] = (h - l).max((h - pc).abs()).max((l - pc).abs());

        let up_move = h - candles[i - 1].high;
        let down_move = candles[i - 1].low - l;
        plus_dm_s[i] = if up_move > down_move && up_move > 0.0 { up_move } else { 0.0 };
        minus_dm_s[i] = if down_move > up_move && down_move > 0.0 { down_move } else { 0.0 };
    }

    let mut tr_w = vec![0.0; n];
    let mut pdm_w = vec![0.0; n];
    let mut mdm_w = vec![0.0; n];
    for i in 1..=period {
        tr_w[period] += tr_s[i];
        pdm_w[period] += plus_dm_s[i];
        mdm_w[period] += minus_dm_s[i];
    }

    for i in period..n {
        if i > period {
            tr_w[i] = tr_w[i - 1] - (tr_w[i - 1] / period as f64) + tr_s[i];
            pdm_w[i] = pdm_w[i - 1] - (pdm_w[i - 1] / period as f64) + plus_dm_s[i];
            mdm_w[i] = mdm_w[i - 1] - (mdm_w[i - 1] / period as f64) + minus_dm_s[i];
        }
        if tr_w[i] > 0.0 {
            let pdi = 100.0 * pdm_w[i] / tr_w[i];
            let mdi = 100.0 * mdm_w[i] / tr_w[i];
            out[i].plus_di = Some(pdi);
            out[i].minus_di = Some(mdi);
            let s = pdi + mdi;
            dx[i] = if s > 0.0 { 100.0 * (pdi - mdi).abs() / s } else { 0.0 };
        }
    }

    let start = period * 2;
    if start >= n {
        return out;
    }
    let mut adx_sum = 0.0;
    for i in (period + 1)..=start {
        adx_sum += dx[i];
    }
    out[start].adx = Some(adx_sum / period as f64);
    for i in (start + 1)..n {
        let prev = out[i - 1].adx.unwrap_or(0.0);
        out[i].adx = Some((prev * (period as f64 - 1.0) + dx[i]) / period as f64);
    }
    out
}

/// ATR with Wilder smoothing
pub fn calc_atr(candles: &[Candle], period: usize) -> Vec<Option<f64>> {
    let n = candles.len();
    let mut atr = vec![None; n];
    if n < period + 1 {
        return atr;
    }

    let mut tr = vec![0.0; n];
    for i in 1..n {
        let h = candles[i].high;
        let l = candles[i].low;
        let pc = candles[i - 1].close;
        tr[i] = (h - l).max((h - pc).abs()).max((l - pc).abs());
    }

    let mut sum = 0.0;
    for i in 1..=period {
        sum += tr[i];
    }
    atr[period] = Some(sum / period as f64);
    for i in (period + 1)..n {
        let prev = atr[i - 1].unwrap_or(0.0);
        atr[i] = Some((prev * (period as f64 - 1.0) + tr[i]) / period as f64);
    }
    atr
}

/// Weekly Volume Profile: PoC, VAH, VAL
pub struct VpLevels {
    pub poc: f64,
    pub vah: f64,
    pub val: f64,
}

pub fn calc_volume_profile(candles: &[Candle], bin_size: f64) -> Option<VpLevels> {
    if candles.is_empty() {
        return None;
    }

    let mut min_p = f64::MAX;
    let mut max_p = f64::MIN;
    for c in candles {
        if c.low < min_p { min_p = c.low; }
        if c.high > max_p { max_p = c.high; }
    }

    let bin_count = ((max_p - min_p) / bin_size).ceil() as usize + 1;
    let mut bins = vec![0.0; bin_count];

    for c in candles {
        let mid = (c.high + c.low) / 2.0;
        let idx = ((mid - min_p) / bin_size) as usize;
        if idx < bin_count { bins[idx] += c.volume; }
    }

    let poc_idx = bins
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(i, _)| i)?;
    let poc = min_p + poc_idx as f64 * bin_size + bin_size / 2.0;

    let total_vol: f64 = bins.iter().sum();
    let target_vol = total_vol * 0.70;
    let mut acc_vol = bins[poc_idx];
    let mut up_idx = poc_idx;
    let mut dn_idx = poc_idx;

    while acc_vol < target_vol && (up_idx + 1 < bin_count || dn_idx > 0) {
        let up_v = if up_idx + 1 < bin_count { bins[up_idx + 1] } else { 0.0 };
        let dn_v = if dn_idx > 0 { bins[dn_idx - 1] } else { 0.0 };
        if up_v >= dn_v && up_idx + 1 < bin_count {
            up_idx += 1;
            acc_vol += bins[up_idx];
        } else if dn_idx > 0 {
            dn_idx -= 1;
            acc_vol += bins[dn_idx];
        } else {
            break;
        }
    }

    Some(VpLevels {
        poc,
        vah: min_p + up_idx as f64 * bin_size + bin_size / 2.0,
        val: min_p + dn_idx as f64 * bin_size + bin_size / 2.0,
    })
}

/// 3D Swing High/Low (last 288 x 15m candles)
pub fn find_3d_swing(candles: &[Candle]) -> Option<(f64, f64)> {
    let window = candles.len().min(288);
    if window == 0 {
        return None;
    }
    let slice = &candles[candles.len() - window..];
    let high = slice.iter().map(|c| c.high).fold(f64::MIN, f64::max);
    let low = slice.iter().map(|c| c.low).fold(f64::MAX, f64::min);
    Some((high, low))
}

/// FIB Golden Pocket (0.618–0.66 retracement)
pub fn fib_golden_pocket(high: f64, low: f64, is_long: bool) -> (f64, f64) {
    let range = high - low;
    if is_long {
        let fib1 = low + range * (1.0 - 0.618);
        let fib2 = low + range * (1.0 - 0.66);
        (fib1.min(fib2), fib1.max(fib2))
    } else {
        let fib1 = high - range * (1.0 - 0.618);
        let fib2 = high - range * (1.0 - 0.66);
        (fib1.max(fib2), fib1.min(fib2))
    }
}
