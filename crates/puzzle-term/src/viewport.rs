//! Device-pixel viewport and tracked top-line anchoring (§7).
//!
//! Scroll position counts pixels above the live bottom. A tracked position,
//! rather than a numeric offset alone, keeps the same text at the top while
//! output appends or reflow assigns new physical line identities. The ring
//! owns eviction; a Gone anchor clamps to its oldest surviving row.
//!
//! Device pixels and document rows are deliberately separate units. Font or
//! DPR changes preserve the row distance before adopting the new cell height.
//! Ring indices select visible content; LineId supplies identity only, because
//! partial-region edits can make its numeric order differ from document order.
//!
//! The alternate screen always has zero history. A screen epoch catches even
//! a primary→alternate→primary transition in a single parser callback, which
//! otherwise ends with the same enum value and could retain an obsolete view.
//! RIS also advances that epoch, making reset semantics explicit to hosts.
//!
//! New-output state is an edge for the future pill, not a byte count. Only
//! actual row arrivals while inspecting history set it; return-to-bottom,
//! input following or a screen reset clears it. Output following is opt-in.
//! The model does not schedule timers or render overlays. Session publishes
//! its changed state and owns presentation requests at the bus boundary.
//!
//! A chosen centre row is presentation policy; it never allocates a tracked mark.
//! Source-to-display projection runs before centring, including user-folded regions.
use puzzle_vt::{
    terminal::{ActiveScreen, Terminal},
    tracked::{LineId, PositionState, TrackedPos},
};
/// User row omissions are derived caches; source ownership stays with handlers.
pub(crate) mod folding;
/// Finite, injected-clock scrolling and row alignment.
mod motion;
/// Geometry and source-driven markers for the future M9 scrollbar overlay.
pub mod scrollbar;
/// Hardware delta kind shared by both platform adapters.
pub use motion::ScrollInput;
/// Animation operations live apart from document anchoring.
mod animation;
/// Absolute navigation targets shared by the bus and platform shortcuts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollTarget {
    /// Oldest retained content.
    Top,
    /// Resume following output.
    Bottom,
    /// Centre a retained line.
    Line(LineId),
    /// Align a retained line with the viewport top.
    LineTop(LineId),
    /// Previous or next OSC 133 prompt in document order.
    Prompt(i32),
    /// Previous or next supplied search-match marker.
    Match(i32),
}
/// Scroll state owned by one Session, independent of time and GPU resources.
pub struct Viewport {
    /// Scroll and silence deadlines contain no platform timer handles.
    motion: motion::Motion,
    /// Lazy reversible display mapping; absent terminals keep their contiguous rows.
    projection: Option<Box<folding::Folds>>,
    /// Host accessibility preference overrides the smooth setting.
    reduced_motion: bool,
    /// Requested smooth policy remains intact when accessibility changes.
    smooth: bool,
    /// Local movement must reach a refresh independently of output settling.
    presentation_pending: bool,
    /// Distance above the live tail in physical pixels.
    scroll_px: f64,
    /// Device-cell height; supplied by the selected text context.
    row_height: f64,
    /// Ring index of the first row, including a clipped top edge.
    top: usize,
    /// The line at the top follows terminal reflow through this registry slot.
    anchor: Option<TrackedPos>,
    /// Screen switches, including an out-and-back pair in one feed, reset view.
    screen_epoch: u64,
    /// Full-width output scrolls are distinct from reflow-assigned identities.
    scroll_total: u64,
    /// History removals also change marker fractions without appending output.
    history_rows: usize,
    /// Primary/alternate changes cannot reuse equal numeric line IDs.
    active: ActiveScreen,
    /// Lines arrived while the user was inspecting history.
    new_output: bool,
    /// Sending real application input resumes following by default.
    pub on_input: bool,
    /// Optional opt-in following on output.
    pub on_output: bool,
    /// Geometry and marker storage remains reusable between source changes.
    pub scrollbar: scrollbar::Scrollbar,
}
impl Default for Viewport {
    /// Starts at the live tail before fonts and history arrive.
    fn default() -> Self {
        Self {
            motion: motion::Motion::default(),
            projection: None,
            reduced_motion: false,
            smooth: true,
            presentation_pending: false,
            scroll_px: 0.0,
            row_height: 1.0,
            top: 0,
            anchor: None,
            screen_epoch: 0,
            scroll_total: 0,
            history_rows: 0,
            active: ActiveScreen::Primary,
            new_output: false,
            on_input: true,
            on_output: false,
            scrollbar: scrollbar::Scrollbar::default(),
        }
    }
}
/// Viewport changes reuse one tracked registry slot and cached scrollbar data.
impl Viewport {
    /// Returns the physical distance above the bottom.
    pub fn scroll_px(&self) -> f64 {
        self.scroll_px
    }
    /// Current device-cell height converts pixel gestures to protocol row steps.
    pub(crate) fn row_height(&self) -> u32 {
        self.row_height as u32
    }
    /// Zero offset means new output remains visible.
    pub fn following(&self) -> bool {
        self.scroll_px == 0.0
    }
    /// Exposes the state used by M9's new-output pill.
    pub fn new_output(&self) -> bool {
        self.new_output
    }
    /// Top ring index, avoiding arithmetic on potentially noncontiguous IDs.
    pub fn top_index(&self) -> usize {
        self.top
    }
    /// Derives whole-row distance from the live screen.
    pub fn offset_rows(&self) -> usize {
        (self.scroll_px / self.row_height).floor() as usize
    }
    /// Converts the physical cell height without moving the anchored content.
    pub fn set_row_height(&mut self, height: u32) {
        // Retaining row distance avoids a DPR change doubling the history jump.
        let rows = self.scroll_px / self.row_height;
        if self.row_height != f64::from(height.max(1)) {
            self.motion.cancel();
        }
        self.row_height = f64::from(height.max(1));
        self.scroll_px = rows * self.row_height;
    }
    /// Positive rows move down toward live output; negative rows move back.
    pub fn scroll(&mut self, term: &mut Terminal, rows: f64) {
        // External API deltas cannot turn clamp arithmetic into NaN.
        self.motion.cancel();
        if rows.is_finite() {
            self.set_pixels(term, self.scroll_px - rows * self.row_height);
        }
    }
    /// Clamps untrusted positions and refreshes the tracked top-line owner.
    pub fn set_pixels(&mut self, term: &mut Terminal, pixels: f64) {
        // History excludes the live grid and is zero on the alternate screen.
        let history = self.display_history(term);
        let limit = history as f64 * self.row_height;
        self.scroll_px = if pixels.is_finite() {
            pixels.clamp(0.0, limit)
        } else {
            0.0
        };
        // A fractional viewport begins in the preceding row, which must be
        // retained for clipping instead of leaving an empty strip at the top.
        // Pixel motion counts displayed rows; the tracker still owns a physical source.
        let display = history.saturating_sub((self.scroll_px / self.row_height).ceil() as usize);
        self.top = self.retained_row(display).unwrap_or(0);
        // Replacing the owner releases its old generation-checked handle.
        self.reanchor(term);
        if self.following() {
            self.new_output = false;
        }
        self.scrollbar.set_position(display, term.size().1, history);
    }
    /// True when the old screen/reset context cannot retain its local markers.
    pub(crate) fn context_changed(&self, term: &Terminal) -> bool {
        self.screen_epoch != term.screen_epoch
    }
    /// Detects source changes that invalidate scrollbar row fractions.
    pub(crate) fn scroll_total_changed(&self, term: &Terminal) -> bool {
        self.scroll_total != term.screen().ring.scroll_total()
            || self.history_rows != term.screen().ring.scrollback_len()
            || self.screen_epoch != term.screen_epoch
    }
    /// Reconciles terminal mutations before a frame or input turn.
    pub fn reconcile(&mut self, term: &mut Terminal, output: bool) {
        let before = self.scroll_px;
        let history = self.display_history(term);
        // Compare transitions, not only the screen value at the end of a feed.
        let switched = self.screen_epoch != term.screen_epoch || self.active != term.active;
        // Reflow allocates fresh IDs but is not new application output.
        let arrived = term.screen().ring.scroll_total() != self.scroll_total;
        if switched || history == 0 || (output && self.on_output) || self.following() {
            if switched || history == 0 || (output && self.on_output) {
                self.motion.cancel();
            }
            self.set_pixels(term, 0.0);
        } else {
            // Resolve the tracked logical point after VT has applied reflow.
            // Gone falls back to the oldest survivor; history bounds row resize.
            let index = self
                .anchor
                .and_then(|handle| match term.tracked.get(handle) {
                    PositionState::Present { line, .. } => term.screen().ring.index_of(line),
                    PositionState::Gone => None,
                })
                .unwrap_or(0);
            let index = self.display_anchor(index).unwrap_or(0).min(history);
            // Preserve sub-row displacement when new lines extend the tail.
            let displacement = self.row_shift();
            self.set_pixels(
                term,
                (history - index) as f64 * self.row_height + displacement,
            );
            if output && arrived && !self.following() {
                self.new_output = true;
            }
        }
        self.motion
            .translate(self.scroll_px - before, history as f64 * self.row_height);
        self.active = term.active;
        self.screen_epoch = term.screen_epoch;
        self.scroll_total = term.screen().ring.scroll_total();
        self.history_rows = term.screen().ring.scrollback_len();
    }
    /// Sends the viewport to a boundary or a centred marker line.
    pub fn navigate(&mut self, term: &mut Terminal, target: ScrollTarget) {
        self.motion.cancel();
        let history = self.display_history(term);
        match target {
            ScrollTarget::Top => self.set_pixels(term, history as f64 * self.row_height),
            ScrollTarget::Bottom => self.set_pixels(term, 0.0),
            ScrollTarget::Line(line) | ScrollTarget::LineTop(line) => {
                let row = if matches!(target, ScrollTarget::LineTop(_)) {
                    0
                } else {
                    term.size().1 / 2
                };
                self.centre_line_at(term, line, row);
            }
            ScrollTarget::Prompt(delta) | ScrollTarget::Match(delta) => {
                if let Some(line) = self.scrollbar.navigate(
                    self.top,
                    delta,
                    matches!(target, ScrollTarget::Prompt(_)),
                ) {
                    if matches!(target, ScrollTarget::Prompt(_)) {
                        // Prompt jumps align the target at the top, so repeated jumps
                        // advance to distinct boundaries rather than reselecting one.
                        if let Some(index) = term.screen().ring.index_of(line) {
                            self.set_pixels(
                                term,
                                history.saturating_sub(self.display_anchor(index).unwrap_or(0))
                                    as f64
                                    * self.row_height,
                            );
                        }
                    } else {
                        self.navigate(term, ScrollTarget::Line(line));
                    }
                }
            }
        }
    }
    /// Position a retained line at a chosen display row, preserving source bounds.
    pub(crate) fn centre_line_at(&mut self, term: &mut Terminal, line: LineId, row: usize) {
        self.motion.cancel();
        let history = self.display_history(term);
        // The source may have been evicted since the request was queued.
        if let Some(index) = term.screen().ring.index_of(line) {
            let top = self
                .display_anchor(index)
                .unwrap_or(0)
                .saturating_sub(row.min(term.size().1.saturating_sub(1)))
                .min(history);
            // Document edges still clamp; exclusions never insert synthetic text rows.
            self.set_pixels(term, (history - top) as f64 * self.row_height);
        }
    }
    /// Future thumb dragging takes a fraction of its available travel.
    pub fn drag_thumb_to(&mut self, term: &mut Terminal, fraction: f64) {
        if fraction.is_finite() {
            // Fraction zero puts the thumb at the document beginning; one
            // consumes all travel and resumes bottom-following.
            let history = self.display_history(term) as f64;
            self.set_pixels(
                term,
                (1.0 - fraction.clamp(0.0, 1.0)) * history * self.row_height,
            );
        }
    }
    /// A track click pages toward the point in device pixels.
    pub fn page_towards(&mut self, term: &mut Terminal, point: f64) {
        // Clicks inside the thumb are inert; track space above/below pages.
        let thumb = self.scrollbar.thumb();
        let direction = if point < thumb[0] {
            -1.0
        } else if point > thumb[0] + thumb[1] {
            1.0
        } else {
            0.0
        };
        self.scroll(term, direction * term.size().1.saturating_sub(1) as f64);
    }
    /// Releases the prior registry handle before owning the new top line.
    fn reanchor(&mut self, term: &mut Terminal) {
        if let Some(handle) = self.anchor.take() {
            term.tracked.release(handle);
        }
        // Followers need no anchor; holding one would pin obsolete content.
        if !self.following()
            && let Some(line) = term.screen().ring.retained(self.top).map(|row| row.id)
        {
            self.anchor = Some(term.tracked.track(line, 0));
        }
    }
}
#[cfg(test)]
/// Synthetic output and ring transformations pin the anchoring contract.
mod tests;
