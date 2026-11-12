//! Native link-restart evidence for finite cadence probes (K26).
//!
//! Start entry and first callback come directly from puzzle-sys. The first
//! selected tick is recorded separately: a restart cannot excuse an extra
//! refresh between callback delivery and drawing. These clocks observe policy;
//! they never keep an idle link running or reserve a frame.
use serde_json::{Value, json};
use std::time::Instant;

/// Cold restart receipts and the first tick selected after the interval starts.
#[derive(Default)]
pub(super) struct Journal {
    /// Restart events include warmup, retained with signed relative timestamps.
    restarts: Vec<(Instant, Instant)>,
    /// Earliest selected callback during the measured interval.
    first_tick: Option<Instant>,
}

/// Warmup and callback selection must not be confused with an idle restart.
#[cfg(test)]
mod tests;

/// Keeps native lifecycle clocks separate from the probe's measurement origin.
impl Journal {
    /// Receipts are rare lifecycle events, never per-cell observations.
    pub(super) fn restarted(&mut self, start: Instant, first: Instant) {
        self.restarts.push((start, first));
    }

    /// The recorder's origin, rather than a guessed warmup duration, gates ticks.
    pub(super) fn tick(&mut self, callback: Instant, origin: Option<Instant>) {
        if origin.is_some_and(|origin| callback >= origin) && self.first_tick.is_none() {
            self.first_tick = Some(callback);
        }
    }

    /// Keep every restart and both clocks so the acceptance reader can prove attribution.
    pub(super) fn json(&self, origin: Instant) -> Value {
        // Warmup restarts precede the origin and must remain visible as negative
        // offsets, rather than saturating to zero and resembling a leading tick.
        let ms = |at: Instant| {
            if at >= origin {
                at.duration_since(origin).as_secs_f64() * 1000.0
            } else {
                -origin.duration_since(at).as_secs_f64() * 1000.0
            }
        };
        // Preserve receipt order and the native duration independently: a slow
        // callback and a delayed frame after that callback are different stages.
        json!({
            "first_tick_ms":self.first_tick.map(ms),
            "restarts":self.restarts.iter().map(|&(start, first)| json!({
                "start_ms":ms(start), "first_callback_ms":ms(first),
                "duration_ms":first.saturating_duration_since(start).as_secs_f64()*1000.0
            })).collect::<Vec<_>>()
        })
    }
}
