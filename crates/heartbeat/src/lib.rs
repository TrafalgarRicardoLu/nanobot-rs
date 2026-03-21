#[derive(Debug, Clone)]
pub struct HeartbeatService {
    interval_ticks: u64,
    last_tick: u64,
}

impl HeartbeatService {
    pub fn new(interval_ticks: u64) -> Self {
        Self {
            interval_ticks: interval_ticks.max(1),
            last_tick: 0,
        }
    }

    pub fn is_due(&self, now_tick: u64) -> bool {
        now_tick.saturating_sub(self.last_tick) >= self.interval_ticks
    }

    pub fn mark_sent(&mut self, now_tick: u64) {
        self.last_tick = now_tick;
    }
}

#[cfg(test)]
mod tests;
