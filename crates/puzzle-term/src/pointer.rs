//! Shared selection gestures and edge-drag timing (§6.9, §7.3).
//!
//! Platforms coalesce motion, but retain signed grid-relative device pixels.
//! Protocol MouseEvent coordinates clamp separately: an outside position must
//! remain measurable for auto-scroll without becoming an invalid xterm report.
//! The anchor uses the terminal's tracked registry through output and reflow.
//! No text is copied and no platform timer is owned by this state machine.
//!
//! The press owns an anchor even before a range exists. A simple release with
//! no intervening movement clears an old selection, but never selects a cell.
//! Word and logical-line presses instead create a range immediately. Their
//! expansion kind remains fixed as the other endpoint moves across the document.
//! Alt selects the same physical column interval on every crossed row.
//! Shift reuses the previous press anchor, including the direction of a prior drag.
//! When no retained press survives, the current selection's first cell is used.
//!
//! Releasing ends the refresh loop but keeps the press anchor for later extension.
//! A new press releases that owner before obtaining its replacement. Source
//! reconciliation also releases it after eviction, a screen change or a rewrite
//! that invalidated selected text. An old handle can never become new content.
//!
//! Edge speed is expressed in rows per second, independently of font/DPR and
//! wheel multiplier. Its sign names document direction, while Viewport converts
//! it to distance above the live tail. At a history boundary it is inert.
//! Time integration belongs to a requested display turn, not every OS callback.
//! A stationary outside pointer therefore moves even without more motion input.
//! A long background pause is bounded to one short step on resume.

use crate::{
    selection::{SelectionCommand, SelectionRange},
    viewport::Viewport,
    vt::{
        Terminal,
        input::{Modifiers, MouseEvent, MouseKind},
        selection::{SelectPoint, SelectionKind},
        tracked::{PositionState, TrackedPos},
    },
};
use web_time::Instant;

/// Text-view boundaries are separate from cell-based application mouse reports.
mod boundary;

/// One platform-neutral pointer sample after host motion coalescing.
#[derive(Clone, Copy, Debug)]
pub struct PointerInput {
    /// Clamped application report with button, click count and modifier state.
    pub event: MouseEvent,
    /// Signed device pixels relative to the grid, preserving outside distance.
    pub position: [f64; 2],
    /// Device-cell geometry used by the same host text context as rendering.
    pub cell: [u32; 2],
}

/// One local drag; a released anchor remains available for Shift extension.
pub(crate) struct Gesture {
    /// Stable press cell or text gap remapped by the shared VT registry.
    anchor: Option<TrackedPos>,
    /// Screen incarnation prevents out-and-back transitions retaining a drag.
    epoch: u64,
    /// Simple drags track gaps; other gestures retain inclusive cell endpoints.
    boundaries: bool,
    /// Live preference snaps only simple endpoints in default trailing padding.
    pub(crate) line_end_snap: bool,
    /// Expansion chosen on press stays fixed throughout its drag.
    kind: SelectionKind,
    /// Left-button ownership ends immediately on release or cancellation.
    active: bool,
    /// A simple click clears selection but does not paint a single cell.
    selected: bool,
    /// Latest signed position and device geometry for stationary edge dragging.
    sample: Option<PointerInput>,
    /// Injected refresh time integrates speed without a separate timer.
    at: Option<Instant>,
}
/// An idle pointer holds no anchor and requests no frames.
impl Default for Gesture {
    /// Starts without a drag, retaining no terminal document references.
    fn default() -> Self {
        Self {
            anchor: None,
            epoch: 0,
            boundaries: false,
            line_end_snap: true,
            kind: SelectionKind::Simple,
            active: false,
            selected: false,
            sample: None,
            at: None,
        }
    }
}

/// All gesture state is session-owned and reusable by desktop and web input.
impl Gesture {
    /// Returns a retained hit using the fractional viewport's displayed rows.
    pub(crate) fn hit(
        input: PointerInput,
        term: &Terminal,
        view: &Viewport,
    ) -> Option<SelectPoint> {
        // External hosts may supply malformed samples; they cannot name a cell.
        if input.position.iter().any(|value| !value.is_finite()) {
            return None;
        }
        // Device geometry matches the fractional shift used by frame assembly.
        // Nonzero fallback also protects direct callers before host validation.
        let [w, h] = input.cell.map(|n| f64::from(n.max(1)));
        let (cols, rows) = term.size();
        // Shift is an integral device offset; a clipped top row remains hittable.
        let y = input.position[1].clamp(0.0, rows as f64 * h - 1.0);
        let row = ((y - f64::from(view.device_shift())) / h).floor() as usize;
        // Horizontal edge dragging selects the nearest real column.
        // Vertical distance drives auto-scroll separately from this clamped hit.
        let column = (input.position[0].max(0.0) / w).floor() as usize;
        // No retained row means the hit belongs to no current document content.
        Some(SelectPoint {
            line: term.screen().ring.retained(view.source_row(row)?)?.id,
            column: column.min(cols - 1),
        })
    }

    /// Begins a local selection or extends the existing press anchor with Shift.
    pub(crate) fn press(
        &mut self,
        term: &mut Terminal,
        point: SelectPoint,
        input: PointerInput,
        range: Option<SelectionRange>,
        now: Instant,
    ) -> SelectionCommand {
        // Alt takes precedence over click expansion: a block is a physical slice.
        // Word and line expansions are performed by the existing selection engine.
        self.kind = if input.event.modifiers.0 & Modifiers::ALT != 0 {
            SelectionKind::Block
        } else {
            match input.event.click_count {
                2 => SelectionKind::Word,
                3.. => SelectionKind::Line,
                _ => SelectionKind::Simple,
            }
        };
        // Only the single-click text-view gesture uses gaps. Word, line and
        // Option rectangles continue to address the cells they historically did.
        self.boundaries = self.kind == SelectionKind::Simple;
        let point = if self.boundaries {
            boundary::hit(term, point, input, self.line_end_snap)
        } else {
            point
        };
        // Shift only extends an existing range; otherwise it begins a normal press.
        let extend = (input.event.modifiers.0 & Modifiers::SHIFT != 0) && range.is_some();
        // Preserve press direction across release, unlike an ordered range’s endpoints.
        // API-created ranges have no press owner and fall back to their start.
        let anchor = if extend {
            self.resolve(term)
                .or(range.map(|r| r.start))
                .unwrap_or(point)
        } else {
            point
        };
        // Resolve before releasing, because the old handle can be recycled below.
        self.cancel(term);
        self.anchor = Some(term.tracked.track(anchor.line, anchor.column));
        // Primary and alternate screens may share IDs but never this incarnation.
        self.epoch = term.screen_epoch;
        // Ownership persists even before the first simple-drag highlight exists.
        self.active = true;
        self.selected = extend || matches!(self.kind, SelectionKind::Word | SelectionKind::Line);
        // Retain the outside position so a stationary pointer can scroll on refresh.
        self.sample = Some(input);
        self.at = Some(now);
        // An ordinary click clears the old highlight; motion creates the first range.
        if self.selected {
            self.request(anchor, point)
        } else {
            SelectionCommand::Clear
        }
    }

    /// Moves an owned drag; hover only updates the semantic pointer hit in Session.
    pub(crate) fn moved(
        &mut self,
        term: &Terminal,
        point: SelectPoint,
        input: PointerInput,
        now: Instant,
    ) -> Option<SelectionCommand> {
        // Hover remains useful to semantic actions, but cannot move an old range.
        if !self.active {
            return None;
        }
        // Gone anchors terminate this update instead of attaching to recycled text.
        let anchor = self.resolve(term)?;
        // Entering an edge begins time integration here, rather than at the press.
        let was_outside = self.speed(term) != 0.0;
        self.sample = Some(input);
        if !was_outside {
            self.at = Some(now);
        }
        // Preserve a rectangle click's existing no-motion behavior. Simple text
        // gaps instead clear themselves when their two boundaries are identical.
        if !self.boundaries
            && !self.selected
            && anchor == point
            && input.event.kind != MouseKind::Move
        {
            return None;
        }
        // Rounding the moving edge again (rather than rounding a delta from the
        // press) gives the same threshold when the user reverses drag direction.
        let point = if self.boundaries {
            boundary::hit(term, point, input, self.line_end_snap)
        } else {
            point
        };
        self.selected = !self.boundaries || anchor != point;
        Some(self.request(anchor, point))
    }

    /// Converts text gaps only for simple drags, preserving other gesture policies.
    fn request(&self, start: SelectPoint, end: SelectPoint) -> SelectionCommand {
        // The command retains edge semantics through interception and recording;
        // converting to inclusive cells belongs to the accepted selection handler.
        if self.boundaries {
            SelectionCommand::Between { start, end }
        } else {
            SelectionCommand::Range {
                kind: self.kind,
                start,
                end,
            }
        }
    }

    /// Ends owned motion immediately; return whether copy-on-select can act.
    pub(crate) fn release(&mut self) -> bool {
        // An unrelated release must not trigger copy-on-select for a retained anchor.
        let copy = self.active && self.selected;
        self.active = false;
        self.at = None;
        copy
    }

    /// Resolves the original anchor without manufacturing a replacement after eviction.
    fn resolve(&self, term: &Terminal) -> Option<SelectPoint> {
        // Reject an out-and-back screen transition before inspecting its handle.
        if self.epoch != term.screen_epoch {
            return None;
        }
        match term.tracked.get(self.anchor?) {
            PositionState::Present { line, column } => Some(SelectPoint { line, column }),
            PositionState::Gone => None,
        }
    }

    /// Source mutation cancels stale drags and returns their last refresh timestamp.
    /// The viewport uses that injected time to settle a fractional displacement.
    pub(crate) fn reconcile(
        &mut self,
        term: &mut Terminal,
        selected: bool,
        shift_selects: bool,
    ) -> Option<Instant> {
        // Current application tracking can claim a drag begun before its mode change.
        let tracking = crate::vt::input::MouseModes::from_terminal(term).tracking;
        let local = tracking == crate::vt::input::MouseTracking::None
            || (shift_selects
                && self
                    .sample
                    .is_some_and(|sample| sample.event.modifiers.0 & Modifiers::SHIFT != 0));
        // A selected-row rewrite is signalled by the selection engine losing its range.
        // Before first motion there is no selected interval to validate.
        if self.anchor.is_some()
            && (!local || self.resolve(term).is_none() || (self.selected && !selected))
        {
            let at = self.at.filter(|_| self.active);
            self.cancel(term);
            return at;
        }
        None
    }

    /// Releases the tracked owner and all motion state without touching selected text.
    /// Returns whether an active drag ended, so the caller can settle the viewport.
    pub(crate) fn cancel(&mut self, term: &mut Terminal) -> bool {
        let active = self.active;
        // Release exactly this owner; selection endpoints own separate handles.
        if let Some(anchor) = self.anchor.take() {
            term.tracked.release(anchor);
        }
        self.active = false;
        self.selected = false;
        self.sample = None;
        self.at = None;
        active
    }

    /// Signed rows/second; negative is above, positive below the grid.
    fn speed(&self, term: &Terminal) -> f64 {
        if !self.active {
            return 0.0;
        }
        let Some(input) = self.sample else {
            return 0.0;
        };
        // Express distance in row heights so Retina does not double drag speed.
        let height = f64::from(input.cell[1].max(1));
        edge_speed(input.position[1], term.size().1 as f64 * height, height)
    }

    /// A boundary or an inside pointer requires no refresh loop (P2).
    pub(crate) fn animating(&self, term: &Terminal, view: &Viewport) -> bool {
        let speed = self.speed(term);
        // The primary history ends at this pixel distance; alternate history is zero.
        // A pointer beyond a reached boundary needs no continuous refresh.
        let limit = view.display_history(term) as f64 * f64::from(view.row_height());
        self.active
            && self.resolve(term).is_some()
            && ((speed < 0.0 && view.scroll_px() < limit)
                || (speed > 0.0 && view.scroll_px() > 0.0))
    }

    /// Advances stationary outside dragging at one injected display timestamp.
    pub(crate) fn advance(
        &mut self,
        term: &mut Terminal,
        view: &mut Viewport,
        now: Instant,
    ) -> Option<SelectionCommand> {
        if !self.animating(term, view) {
            self.at = Some(now);
            return None;
        }
        // Save this turn before integrating, making repeated timestamps inert.
        let previous = self.at.replace(now)?;
        // A resumed background window must not integrate minutes of missed frames.
        let seconds = now
            .saturating_duration_since(previous)
            .as_secs_f64()
            .min(0.05);
        let rows = self.speed(term) * seconds;
        view.drag(term, rows);
        let input = self.sample?;
        // The same stationary screen position now names a different document row.
        let point = Self::hit(input, term, view)?;
        self.moved(term, point, input, now)
    }
}

/// Distance in row heights controls speed, capped to keep an errant sample bounded.
pub(crate) fn edge_speed(y: f64, bottom: f64, height: f64) -> f64 {
    if !y.is_finite() || !bottom.is_finite() || height <= 0.0 {
        return 0.0;
    }
    // Inside and exactly on the grid boundary mean no edge movement.
    // Negative distance is above; positive distance below retains its direction.
    let outside = if y < 0.0 {
        y
    } else if y > bottom {
        y - bottom
    } else {
        return 0.0;
    };
    outside.signum() * (3.0 + outside.abs() / height * 12.0).min(120.0)
}
