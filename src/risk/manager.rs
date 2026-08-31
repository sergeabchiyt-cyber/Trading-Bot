use crate::config::Config;

pub struct PositionManager {
    pub entry_price: f64,
    pub initial_sl: f64,
    pub tp: f64,
    pub qty: f64,
    pub is_long: bool,
    pub high_water_mark: f64,
    pub sl_moved_to_be: bool,
    pub trailing_active: bool,
    pub initial_risk_per_unit: f64,
}

impl PositionManager {
    pub fn new(
        cfg: &Config,
        equity: f64,
        entry_price: f64,
        initial_sl: f64,
        tp: f64,
        is_long: bool,
    ) -> Self {
        let risk_per_unit = (entry_price - initial_sl).abs();
        let risk_amount = equity * cfg.risk_pct;
        let qty = risk_amount / risk_per_unit;

        Self {
            entry_price,
            initial_sl,
            tp,
            qty,
            is_long,
            high_water_mark: entry_price,
            sl_moved_to_be: false,
            trailing_active: false,
            initial_risk_per_unit: risk_per_unit,
        }
    }

    /// Returns the current SL price after applying BE and trailing logic
    pub fn update_sl(&mut self, cfg: &Config, current_price: f64) -> f64 {
        let r_multiple = self.calc_r(current_price);

        // Move to near-BE at 1.0R
        if r_multiple >= 1.0 && !self.sl_moved_to_be {
            self.sl_moved_to_be = true;
            let be_offset = self.entry_price * 0.0005; // 0.05%
            self.initial_sl = if self.is_long {
                self.entry_price + be_offset
            } else {
                self.entry_price - be_offset
            };
        }

        // Activate trailing at 1.2R
        if r_multiple >= cfg.trail_activation_r && !self.trailing_active {
            self.trailing_active = true;
        }

        // Update high water mark and trail
        if self.is_long {
            if current_price > self.high_water_mark {
                self.high_water_mark = current_price;
            }
            if self.trailing_active {
                let trail_sl = self.high_water_mark * (1.0 - cfg.trail_pullback_pct);
                if trail_sl > self.initial_sl {
                    self.initial_sl = trail_sl;
                }
            }
        } else {
            if current_price < self.high_water_mark {
                self.high_water_mark = current_price;
            }
            if self.trailing_active {
                let trail_sl = self.high_water_mark * (1.0 + cfg.trail_pullback_pct);
                if trail_sl < self.initial_sl {
                    self.initial_sl = trail_sl;
                }
            }
        }

        self.initial_sl
    }

    fn calc_r(&self, price: f64) -> f64 {
        if self.initial_risk_per_unit == 0.0 { return 0.0; }
        if self.is_long {
            (price - self.entry_price) / self.initial_risk_per_unit
        } else {
            (self.entry_price - price) / self.initial_risk_per_unit
        }
    }

    pub fn should_exit(&self, price: f64) -> bool {
        if self.is_long {
            price <= self.initial_sl || price >= self.tp
        } else {
            price >= self.initial_sl || price <= self.tp
        }
    }
}
