//! Injected-clock scrolling; only notches and the final row snap add inertia.
//!
//! Pixel input already contains OS inertia, so integrating it a second time
//! would make a trackpad lag. A notch instead retargets from the sampled
//! position to the accumulated destination. Snap waits for silence, rather
//! than starting after every frame. No clock is read inside this model.
use std::time::Duration;
use web_time::Instant;

/// Preserves the distinction between an OS pixel stream and wheel notches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollInput {
    /// Device-pixel movement; positive moves toward the live tail.
    Pixels(f64),
    /// Row movement from notches; positive moves toward the live tail.
    Rows(f64),
}
/// One ease-out segment with a fixed destination in device pixels.
struct Tween {
    /// Position sampled when this segment starts, avoiding a retarget jump.
    from: f64,
    /// Accumulated destination, clamped before starting the segment.
    to: f64,
    /// Host-supplied monotonic origin.
    started: Instant,
    /// Notches use 120 ms; the final alignment uses 100 ms (§7.3).
    duration: Duration,
}
/// Finite animation state; the platform sleeps once its deadlines disappear.
pub(super) struct Motion {
    /// Active ease-out, absent between pixel events and while waiting to snap.
    tween: Option<Tween>,
    /// One silence deadline, reset by each actual input event.
    snap_at: Option<Instant>,
    /// User multiplier applies to both input kinds.
    pub(super) multiplier: f64,
    /// Reduced motion and the smooth setting both forbid animation.
    pub(super) smooth: bool,
    /// Alignment can be disabled independently of notch easing.
    pub(super) snap: bool,
}
/// The inactive state schedules neither a deadline nor a display turn.
impl Default for Motion {
    /// Matches the settings schema, without a timer until input arrives.
    fn default() -> Self {
        Self {
            tween: None,
            snap_at: None,
            multiplier: 3.0,
            smooth: true,
            snap: true,
        }
    }
}
/// Samples exact positions; the viewport clamps and owns tracked document anchors.
impl Motion {
    /// Samples one segment, retaining its exact endpoint on completion.
    pub(super) fn sample(&mut self, position: f64, height: f64, now: Instant) -> f64 {
        // Retain pixel input unchanged when no manufactured inertia is active.
        let mut result = position;
        if let Some(tween) = &self.tween {
            // Saturation handles a clock sample before the segment origin safely.
            // Late refreshes take the endpoint, rather than extrapolating past it.
            let t = (now.saturating_duration_since(tween.started).as_secs_f64()
                / tween.duration.as_secs_f64())
            .min(1.0);
            // Cubic ease-out starts promptly and arrives without a hard stop.
            result = tween.from + (tween.to - tween.from) * (1.0 - (1.0 - t).powi(3));
            // Completion removes the source of further animation frame requests.
            if t == 1.0 {
                self.tween = None;
            }
        }
        if self.snap_at.is_some_and(|deadline| now >= deadline) {
            // Consume silence once; the snap tween must not start another timer.
            self.snap_at = None;
            // Nearest-row rounding uses the current device-cell height.
            let target = (result / height).round() * height;
            if self.smooth && target != result {
                // Start at the current sampled point, even after a late frame.
                self.start(result, target, now, 100);
            } else {
                result = target;
            }
        }
        result
    }
    /// Accepts input after sampling the old segment at the same timestamp.
    pub(super) fn input(
        &mut self,
        position: f64,
        height: f64,
        limit: f64,
        input: ScrollInput,
        now: Instant,
    ) -> f64 {
        // Rows convert once; OS pixels already name physical displacement.
        let delta = match input {
            ScrollInput::Pixels(px) => px,
            ScrollInput::Rows(rows) => rows * height,
        };
        // A forged NaN or infinity cannot create a perpetual animation timer.
        if !delta.is_finite() || delta == 0.0 {
            return position;
        }
        // Retarget from the old destination so repeated notches accumulate.
        // Pixel events instead start at the latest sampled position, preserving OS inertia.
        let base = if matches!(input, ScrollInput::Rows(_)) {
            self.tween.as_ref().map_or(position, |tween| tween.to)
        } else {
            position
        };
        // Clamp before creating endpoints; interpolation can never overshoot history.
        let target = (base - delta * self.multiplier).clamp(0.0, limit);
        // New input replaces both the prior segment and the previous silence timer.
        self.cancel();
        // A clamped boundary gesture changes nothing and must not arm an idle timer.
        if target == position {
            return position;
        }
        if !self.smooth {
            // Accessibility requests a direct jump, including row alignment.
            return if self.snap {
                (target / height).round() * height
            } else {
                target
            };
        }
        // Only smooth input waits for silence; direct policies have no delayed work.
        if self.snap {
            self.snap_at = Some(now + Duration::from_millis(150));
        }
        if matches!(input, ScrollInput::Rows(_)) && target != position {
            self.start(position, target, now, 120);
            position
        } else {
            target
        }
    }
    /// Drag release arms alignment once, without adding a second inertia model.
    /// Drag release adds a silence deadline without inventing a wheel delta.
    pub(super) fn finish_drag(&mut self, now: Instant) {
        self.cancel();
        if self.snap {
            self.snap_at = Some(now + Duration::from_millis(150));
        }
    }
    /// Builds a segment without allocating or reading the real clock.
    fn start(&mut self, from: f64, to: f64, started: Instant, millis: u64) {
        // One small owned segment keeps this steady input path allocation-free.
        self.tween = Some(Tween {
            from,
            to,
            started,
            duration: Duration::from_millis(millis),
        });
    }
    /// Anchored output shifts both endpoints by the same physical displacement.
    pub(super) fn translate(&mut self, delta: f64, limit: f64) {
        // Appended lines move the live tail without changing the inspected content.
        if let Some(tween) = &mut self.tween {
            tween.from = (tween.from + delta).clamp(0.0, limit);
            tween.to = (tween.to + delta).clamp(0.0, limit);
        }
    }
    /// Explicit navigation, screen switches and following cancel old inertia.
    pub(super) fn cancel(&mut self) {
        self.tween = None;
        self.snap_at = None;
    }
    /// True while a refresh must sample motion, including a due snap start.
    pub(super) fn active(&self, now: Instant) -> bool {
        self.tween.is_some() || self.snap_at.is_some_and(|deadline| deadline <= now)
    }
    /// Waiting for silence needs one timer, not a stream of idle frames.
    pub(super) fn deadline(&self) -> Option<Instant> {
        self.snap_at
    }
}
