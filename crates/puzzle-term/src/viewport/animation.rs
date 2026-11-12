//! Clock-driven movement layered on tracked document anchoring (§7.3).
//!
//! Presentation uses integral device pixels, while the model retains exact
//! easing positions. Rounding only at assembly avoids drift from many small
//! trackpad deltas. Waiting for the silence deadline requests no frames.
use super::{ScrollInput, Viewport};
use puzzle_vt::Terminal;
use web_time::Instant;
/// Animation shares the same clamp and tracked anchor as direct navigation.
impl Viewport {
    /// Signed displacement of the first retained row relative to the grid top.
    pub fn row_shift(&self) -> f64 {
        // Distance is measured above the bottom; a partial top row starts above zero.
        let fraction = self.scroll_px % self.row_height;
        if fraction == 0.0 {
            0.0
        } else {
            fraction - self.row_height
        }
    }
    /// Applies whole-device-pixel displacement to cached row-local geometry.
    pub fn device_shift(&self) -> i32 {
        self.row_shift().round() as i32
    }
    /// Captures the clipped row at the bottom as well as the top.
    pub fn visible_count(&self, rows: usize) -> usize {
        // A whole-row view needs exactly the VT row count; partial views need one more.
        let wanted = rows + usize::from(self.row_shift() != 0.0);
        // Folded documents may contain fewer rows than the fixed PTY grid.
        // Fractional edges still copy at most one additional visible source row.
        // Clamp in display order; physical history length includes hidden rows.
        self.projection.as_ref().map_or(wanted, |folds| {
            wanted.min(
                folds
                    .rows
                    .len()
                    .saturating_sub(self.display_anchor(self.top).unwrap_or(0)),
            )
        })
    }
    /// Samples old motion before retargeting it with a hardware input delta.
    pub fn wheel(&mut self, term: &mut Terminal, input: ScrollInput, now: Instant) {
        // Retargeting first samples the prior segment, avoiding position discontinuity.
        self.advance(term, now);
        // Alternate screens naturally clamp at zero because they have no history.
        let limit = self.display_history(term) as f64 * self.row_height;
        let next = self
            .motion
            .input(self.scroll_px, self.row_height, limit, input, now);
        // A notch can start at the same position and still require its first refresh.
        self.presentation_pending |= next != self.scroll_px || self.motion.active(now);
        self.set_pixels(term, next);
    }
    /// Selection edge movement uses the existing clamp/anchor and presentation edge.
    pub(crate) fn drag(&mut self, term: &mut Terminal, rows: f64) {
        // Continuous edge speed replaces any earlier notch or snap interpolation.
        self.motion.cancel();
        let old = self.scroll_px;
        self.set_pixels(term, old - rows * self.row_height);
        // Hitting a document boundary must not keep requesting unchanged frames.
        self.presentation_pending |= old != self.scroll_px;
    }
    /// Release uses the same silence and row-snap timeline as pixel wheel input.
    /// Drag integration leaves no wheel delta or tween endpoint to extrapolate.
    pub(crate) fn finish_drag(&mut self, term: &mut Terminal, now: Instant) {
        self.motion.cancel();
        // Exact row alignment needs neither a silence timer nor a snap segment.
        if self.scroll_px % self.row_height == 0.0 {
            return;
        }
        // Reduced motion follows the same nearest-row destination immediately.
        if !self.motion.smooth && self.motion.snap {
            self.set_pixels(
                term,
                (self.scroll_px / self.row_height).round() * self.row_height,
            );
        } else {
            // Enabled snapping starts after silence; snap-off leaves the fraction.
            self.motion.finish_drag(now);
        }
    }
    /// Moves at an injected refresh timestamp; idle calls are inert.
    pub fn advance(&mut self, term: &mut Terminal, now: Instant) -> bool {
        // There is no system clock or platform timer inside this deterministic step.
        let next = self.motion.sample(self.scroll_px, self.row_height, now);
        let changed = next != self.scroll_px;
        if changed {
            self.set_pixels(term, next);
        }
        changed
    }
    /// Local presentation bypasses only the output burst's settle delay.
    pub fn animation_frame(&self, now: Instant) -> bool {
        self.presentation_pending || self.motion.active(now)
    }
    /// Clears the accepted frame edge; active motion still requests refreshes.
    pub(crate) fn presented(&mut self) {
        // Clearing the edge does not remove a still-running interpolation segment.
        self.presentation_pending = false;
    }
    /// True while interpolation needs the next refresh.
    pub(crate) fn animating(&self, now: Instant) -> bool {
        self.motion.active(now)
    }
    /// One silence timer starts snapping without polling an idle display.
    pub(crate) fn snap_deadline(&self) -> Option<Instant> {
        self.motion.deadline()
    }
    /// Applies validated live settings, keeping the host motion preference.
    pub(crate) fn configure(&mut self, multiplier: f64, smooth: bool, snap: bool) {
        // Keep the requested policy so removing reduced motion restores user intent.
        self.smooth = smooth;
        self.motion.multiplier = multiplier;
        self.motion.smooth = smooth && !self.reduced_motion;
        self.motion.snap = snap;
        if !self.motion.smooth || !snap {
            self.motion.cancel();
        }
    }
    /// A platform preference never modifies the persisted user setting.
    pub(crate) fn reduced_motion(&mut self, enabled: bool) {
        // Hosts report this media preference; it is never written into user layers.
        self.reduced_motion = enabled;
        self.motion.smooth = self.smooth && !enabled;
        if enabled {
            self.motion.cancel();
        }
    }
}

#[cfg(test)]
/// The clock-driven timelines protect motion and idle behavior.
mod tests;
