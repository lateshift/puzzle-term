//! Host UI is presentation metadata, independent of padding, rows and PTY geometry.
//!
//! The owner validates once and shares an immutable list with each frame slot.
//! Navigation chooses the centre of the largest unobscured row band; source rows
//! and tracked matches are unchanged. Normal surfaces keep their original centre.
//!
//! Exclusions remain view-relative across reflow and display moves; the host
//! replaces them when its native controls move. Keeping unclipped coordinates
//! allows the same list to remain meaningful when a window grows again.
//! A frame slot shares immutable rectangles, so a newer host update cannot move
//! the controls or hit regions of an older presented frame.
//!
//! Rectangle admission is atomic: bad native input cannot erase a valid list.
//! Equal lists do not request frames, so native layout notifications stay idle.
//! The shared allocation is optional; ordinary surfaces allocate no UI metadata.
//! UI coverage is never input to Terminal::resize, reflow or transport::resize.
//! Default padding and producer-gutter geometry therefore stay identical.
//!
//! First-match ranking checks the candidate's occupied columns, including a wide
//! glyph's spacer. A covered result can still be chosen as the fallback and then
//! centred in a free band. Text scanning and tracked result ownership are unchanged.
//! Explicit steps share that centring path with deferred immediate-Enter steps.
//!
//! Navigation can move vertically only: search never pans or edits terminal rows.
//! Candidate bands are evaluated at the match's columns, so a narrow toolbar on
//! the other side cannot exclude otherwise usable space. Wrapped matches reserve
//! their first-row suffix through the edge; ordinary line navigation uses full rows.
//! The longest band preserves surrounding context rather than parking on an edge.
//! No amount of metadata can move beyond retained document bounds; Viewport clamps.
//! A completely covered grid retains its normal target until the host clears space.
//! These decisions run only on navigation, never during printing or row assembly.
use super::Session;
use crate::{
    frame_builder::policy::FramePolicy,
    vt::{Terminal, search::FindMatch},
};
use puzzle_overlay::{geometry::Rect, provider::EventKinds};

impl Session {
    /// Replace atomically; a malformed host list leaves the previous list intact.
    pub(super) fn set_occluded_rects(&mut self, rects: Vec<Rect>) -> Result<(), String> {
        if !puzzle_overlay::occlusion::valid(&rects) {
            return Err("invalid or oversized host occlusion list".into());
        }
        let old = self.model_overlays.occluded_rects.as_deref().unwrap_or(&[]);
        // Repeated native layout callbacks must not create idle rendering work.
        if old == rects {
            return Ok(());
        }
        // None is the allocation-free ordinary-surface path, including after clear.
        self.model_overlays.occluded_rects = (!rects.is_empty()).then(|| rects.into());
        self.overlay_changed(EventKinds::GEOMETRY);
        self.needs_frame = true;
        self.services.waker.request_frame();
        Ok(())
    }
}

/// Logical grid coordinates use the same snapped metrics, inset and gutter as ink.
fn cell_rect(policy: &FramePolicy, column: usize, row: usize, columns: usize) -> Rect {
    // FramePolicy holds device-pixel metrics; divide once into the host's point space.
    let dpr = policy.dpr.max(f32::EPSILON);
    let [width, height] = [
        policy.metrics.width_px as f32 / dpr,
        policy.metrics.height_px as f32 / dpr,
    ];
    let inset = policy.padding as f32 / dpr;
    // Only X includes producer UI cells; Y begins at ordinary outer padding.
    Rect {
        origin: [
            inset + (column + usize::from(policy.gutter_columns)) as f32 * width,
            inset + row as f32 * height,
        ],
        size: [columns as f32 * width, height],
    }
}

/// Initial selection treats a covered first-row match as offscreen, not visible UI.
pub(super) fn visible_match(
    term: &Terminal,
    row: usize,
    hit: &FindMatch,
    policy: Option<&FramePolicy>,
    rects: &[Rect],
) -> bool {
    let Some(policy) = policy.filter(|_| !rects.is_empty()) else {
        return true;
    };
    // Include the wide spacer at the last matched cluster, as match fills do.
    let end = if hit.start.line == hit.end.line {
        let width = term
            .buffer()
            .line(hit.end.line)
            .and_then(|line| line.cell(hit.end.column))
            .map_or(1, |cell| cell.width.max(1));
        hit.end.column + usize::from(width)
    } else {
        term.size().0
    };
    // A soft-wrapped match reaches the row edge before continuing on its next row.
    let bounds = cell_rect(
        policy,
        hit.start.column,
        row,
        end.saturating_sub(hit.start.column),
    );
    // A covered match can still be selected as the offscreen fallback and revealed.
    rects.iter().all(|rect| !bounds.overlaps(*rect))
}

/// A free band for the current match keeps unrelated UI at other columns harmless.
pub(super) fn centre_row(
    policy: Option<&FramePolicy>,
    rects: &[Rect],
    columns: usize,
    rows: usize,
    span: Option<crate::selection::SelectionRange>,
) -> usize {
    let Some(policy) = policy.filter(|_| !rects.is_empty()) else {
        return rows / 2;
    };
    let (column, end) = span.map_or((0, columns), |range| {
        (
            range.start.column,
            if range.start.line == range.end.line {
                range.end.column + 1
            } else {
                columns
            },
        )
    });
    let (mut run, mut best) = (0, (0, 0));
    // Longest contiguous space gives the result context above and below its centre.
    // Physical row visits depend on visible height, never retained history size.
    for row in 0..rows {
        let bounds = cell_rect(policy, column, row, end.saturating_sub(column));
        if rects.iter().all(|rect| !bounds.overlaps(*rect)) {
            run += 1;
            if run > best.1 {
                best = (row + 1 - run, run);
            }
        } else {
            run = 0;
        }
    }
    // A fully covered viewport has no feasible centre; retain normal clamping.
    if best.1 == 0 {
        rows / 2
    } else {
        best.0 + best.1 / 2
    }
}
