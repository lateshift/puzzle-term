//! One-finger scrolling and finite kinetic continuation (§12.2).
//!
//! The browser supplies device pixels and monotonic milliseconds. This model
//! owns gesture velocity, not DOM registrations or a clock, so identical samples
//! have deterministic results. Momentum requests ticks only until it decays.
//! New presses cancel old inertia; cancellation and reduced motion end it at once.
//! A second finger is ignored by the adapter instead of synthesizing mouse input.

//!
//! Downward finger motion has a negative wheel delta, revealing older content.
//! Samples already use device pixels, so DPR is not applied again here.
//! Recent positive elapsed time estimates velocity; a stationary tap stays zero.
//! A stale release cannot relaunch the velocity of an earlier drag sample.
//! Exponential decay integrates actual elapsed time rather than assuming 60 Hz.
//! Below eight pixels per second it ends and requests no more display callbacks.
//! Background suspension integrates at most 32 ms to avoid a giant resume jump.
//! Only the captured ID may move or release; a second finger cannot steal it.
//! A new touch press cancels prior inertia before its first displacement.
//! Release alone does not move text; only drag samples and momentum ticks do.
//! Negative or nonfinite timestamp input cannot produce an unbounded fling.
//! Velocity is capped at three device pixels per millisecond for hostile samples.
//! The adapter stops continuation at history boundaries and on disposal.
//! The capped pause rule prevents hidden pages from integrating an entire sleep.
//! Reduced motion permits direct dragging while suppressing its momentum tail.
//! The cutoff is a finite stopping rule, avoiding an asymptotic idle rAF loop.
//! Touch state contains no selected text, terminal handles or platform callbacks.
//! Selection and application mouse routing remain separate from this gesture.
//! The core receives only the resulting local physical scroll deltas.
//! All state is copied numbers, keeping this model Send and platform neutral.
/// A single touch owns its pointer ID until release, then a short momentum tail.
#[derive(Clone, Copy, Default)]
pub struct Touch {
    /// Captured pointer identity; unrelated fingers cannot move the viewport.
    id: Option<i32>,
    /// Last physical position and event time for estimating drag velocity.
    sample: Option<(f64, f64)>,
    /// Device pixels per millisecond, in wheel's down-positive convention.
    velocity: f64,
    /// Last momentum tick; None means no display callback is required.
    tick: Option<f64>,
}
/// Touch processing does no allocation and never accesses application modes.
impl Touch {
    /// Starts one finger, canceling prior momentum; returns false for another finger.
    pub fn press(&mut self, id: i32, y: f64, now: f64) -> bool {
        if self.id.is_some() || !y.is_finite() || !now.is_finite() {
            return false;
        }
        self.cancel();
        self.id = Some(id);
        self.sample = Some((y, now));
        true
    }
    /// Returns local physical displacement; dragging paper down reveals older lines.
    pub fn moved(&mut self, id: i32, y: f64, now: f64) -> Option<f64> {
        if self.id != Some(id) || !y.is_finite() || !now.is_finite() {
            return None;
        }
        let (previous, at) = self.sample?;
        let pixels = previous - y;
        let elapsed = now - at;
        // Invalid or very stale timestamps must not manufacture an inertia launch.
        self.velocity = if (1.0..=100.0).contains(&elapsed) {
            (pixels / elapsed).clamp(-3.0, 3.0)
        } else {
            0.0
        };
        self.sample = Some((y, now));
        Some(pixels)
    }
    /// Releases this finger; reduced motion or a pause suppresses momentum.
    pub fn release(&mut self, id: i32, now: f64, reduced: bool) {
        if self.id != Some(id) {
            return;
        }
        self.id = None;
        let fresh = self
            .sample
            .is_some_and(|(_, at)| (0.0..=100.0).contains(&(now - at)));
        self.sample = None;
        self.tick = (!reduced && fresh && self.velocity.abs() >= 0.008).then_some(now);
    }
    /// Integrates decay at a refresh; an early timestamp yields no displacement.
    pub fn advance(&mut self, now: f64) -> Option<f64> {
        let at = self.tick?;
        if !now.is_finite() {
            self.cancel();
            return None;
        }
        // rAF's timestamp names the refresh start, not callback execution.
        // A release delivered earlier in this turn can therefore be newer.
        // Wait for the next refresh without erasing its valid momentum.
        if now < at {
            return None;
        }
        // Background suspension cannot fling several seconds of accumulated motion.
        let elapsed = (now - at).min(32.0);
        let decay = (-elapsed / 180.0).exp();
        let pixels = self.velocity * 180.0 * (1.0 - decay);
        self.velocity *= decay;
        self.tick = (self.velocity.abs() >= 0.008).then_some(now);
        Some(pixels)
    }
    /// Whether another momentum tick is needed; idle has no polling callback.
    pub fn animating(&self) -> bool {
        self.tick.is_some()
    }
    /// Releases ownership and velocity on disposal, pointer cancellation or a boundary.
    pub fn cancel(&mut self) {
        *self = Self::default();
    }
}
#[cfg(test)]
/// Injected samples protect direction, ownership, decay and reduced motion.
mod tests;
