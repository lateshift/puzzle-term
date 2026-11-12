//! Background Canvas turns spend remaining refresh time rather than a fixed quantum.
//!
//! A host sample excludes elastic work. The phase anchors all source tasks to
//! one shared refresh horizon; elapsed earlier work reduces later allowance.
//! Late tasks retarget the next reachable edge, capped to one period. Timers
//! become eligible there even on busy pages; no idle callback priority is used.
use std::time::Duration;
use web_time::Instant;

/// Refresh evidence contains no glyph counters, deadlines tuned to a fixture or JS handles.
#[derive(Default)]
pub(super) struct Host {
    /// The latest real rAF begins a refresh-sized scheduling horizon.
    tick: Option<Instant>,
    /// The runtime measures this period from refresh-sized rAF intervals.
    period: Duration,
    /// Inelastic frame work must still fit beside a source task.
    cost: Duration,
}
impl Host {
    /// Stores actual host observations; cold glyphs cannot expand the base cost.
    pub(super) fn update(&mut self, tick: Instant, period: Duration, cost: Duration) {
        self.tick = Some(tick);
        self.period = period;
        self.cost = cost;
    }
    /// Returns current allowance, next eligibility edge and largest ordinary task allowance.
    pub(super) fn window(&self, now: Instant) -> (Duration, Instant, Duration) {
        let reserve = self.cost + Duration::from_millis(1);
        // Background work spends unused time after paint; it is not another
        // frame's inline allowance. Reserve the base cost and margin once.
        let maximum = self.period.saturating_sub(reserve);
        let Some(tick) = self.tick.filter(|_| !self.period.is_zero()) else {
            // Work submitted before its first host observation must first yield.
            return (Duration::ZERO, now, Duration::ZERO);
        };
        let mut edge = tick + self.period;
        if now >= edge {
            // A lost edge never creates permanent zero slack or an unbounded allowance.
            let turns = now.saturating_duration_since(edge).as_nanos() / self.period.as_nanos() + 1;
            edge += self
                .period
                .saturating_mul(u32::try_from(turns).unwrap_or(u32::MAX));
        }
        // Multiple source tasks consume the same wall-clock horizon. Only newly
        // arriving refresh evidence can replenish the time they already spent.
        let left = edge.saturating_duration_since(now).min(self.period);
        (maximum.min(left.saturating_sub(reserve)), edge, maximum)
    }
}

/// Injected clocks exercise busy-page fairness without browser timing assertions.
#[cfg(test)]
mod tests;
