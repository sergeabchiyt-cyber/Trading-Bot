pub enum VpLevelKind { PoC, VAH, VAL }

pub struct VpLevel {
    pub price: f64,
    pub kind: VpLevelKind,
    pub taps: u32,
    pub confirmed_by_4h: bool,
}

impl VpLevel {
    pub fn is_high_prob(&self) -> bool {
        self.taps > 1 && self.confirmed_by_4h
    }
    
    pub fn record_tap(&mut self, confirmed_4h: bool) {
        self.taps += 1;
        if (confirmed_4h) { self.confirmed_by_4h = true; }
    }
}
