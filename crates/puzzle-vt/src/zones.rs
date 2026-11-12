//! Bounded shell-integration marks from OSC 133 and OSC 633 (§6.11).
//!
//! A mark records a stable LineId rather than a current viewport row.
//! Full-width scrolling can move its row without changing the mark. The
//! list keeps only the most recent ten thousand transitions; older marks
//! are evicted before untrusted shell output can grow memory indefinitely.
//!
//! Each mark owns one tracked-position handle. Reflow updates the registry
//! once, then sync copies its coordinates into the public borrowed records.
//! Releasing an old boundary also releases its registry slot, so repeatedly
//! printing prompts cannot leak marker owners beyond the 10,000-mark cap.
//! RIS releases every owner before discarding history. Evicted endpoints
//! remain Gone and are filtered by BufferView rather than resurrected under
//! a later line with unrelated content.

use crate::tracked::{LineId, PositionState, TrackedPos, TrackedPositions};
use std::collections::VecDeque;

/// Which boundary in a prompt/command/output cycle was reported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZoneKind {
    /// The shell began printing a prompt (OSC 133 A).
    Prompt,
    /// The shell began reading command input (OSC 133 B).
    Input,
    /// The shell began printing command output (OSC 133 C).
    Output,
    /// The shell finished a command (OSC 133 D).
    Finished,
}

/// One line-anchored semantic boundary used by later selection and overlays.
#[derive(Clone, Debug)]
pub struct ZoneMark {
    /// Stable command identity, assigned by Zones rather than supplied by OSC.
    pub id: u64,
    /// Monotonic line identity at the mark's cursor location.
    pub line: LineId,
    /// Zero-based grid column at the reported boundary.
    pub column: usize,
    /// Transition type supplied by shell integration.
    pub kind: ZoneKind,
    /// Exit status for a finished command when the shell supplied it.
    pub exit: Option<i32>,
}

/// The newest 10,000 shell marks, independent of any platform clock.
#[derive(Clone, Debug, Default)]
pub struct Zones {
    /// Oldest-first bounded queue for the shell's reported transitions.
    marks: VecDeque<ZoneMark>,
    /// A prompt starts a new command; reset never reuses old host identities.
    next_id: u64,
    /// Parallel owners let public mark values remain cheap borrowed records.
    anchors: VecDeque<Option<TrackedPos>>,
}

impl Zones {
    /// Copies frame metadata while preserving every warmed backing allocation.
    /// Derived Clone replaces nested buffers; detached provider snapshots reuse them.
    pub fn snapshot_from(&mut self, source: &Self) {
        // Only a new metadata high-water mark can grow these owned buffers (P1).
        self.marks.clone_from(&source.marks);
        self.anchors.clone_from(&source.anchors);
        self.next_id = source.next_id;
    }

    /// Stores a shell boundary using the shared reflow/eviction registry.
    pub fn push(&mut self, mut mark: ZoneMark, tracked: &mut TrackedPositions) -> u64 {
        // Missing A boundaries still begin a command; D closes its identity.
        if mark.kind == ZoneKind::Prompt
            || self
                .marks
                .back()
                .is_none_or(|m| m.kind == ZoneKind::Finished)
        {
            self.next_id = self.next_id.saturating_add(1);
        }
        mark.id = self.next_id;
        if self.marks.len() == 10_000 {
            self.marks.pop_front();
            if let Some(Some(handle)) = self.anchors.pop_front() {
                tracked.release(handle);
            }
        }
        // Only shell transitions allocate owners; ordinary printing stays direct.
        self.anchors
            .push_back(Some(tracked.track(mark.line, mark.column)));
        let id = mark.id;
        self.marks.push_back(mark);
        // Session timestamps this identity without giving VT access to a clock.
        id
    }

    /// Refreshes public mark coordinates after the ring remaps its anchors.
    pub fn sync(&mut self, tracked: &TrackedPositions) {
        for (mark, handle) in self.marks.iter_mut().zip(&self.anchors) {
            if let Some(handle) = handle {
                match tracked.get(*handle) {
                    PositionState::Present { line, column } => {
                        mark.line = line;
                        mark.column = column;
                    }
                    // Gone IDs are filtered by BufferView; never point at fresh rows.
                    PositionState::Gone => {
                        mark.line = LineId(u64::MAX);
                    }
                }
            }
        }
    }

    /// Releases all registry owners when RIS discards shell history.
    pub fn clear(&mut self, tracked: &mut TrackedPositions) {
        for handle in self.anchors.drain(..).flatten() {
            tracked.release(handle);
        }
        self.marks.clear();
    }

    /// Returns retained marks without copying their line positions.
    pub fn iter(&self) -> impl Iterator<Item = &ZoneMark> {
        self.marks.iter()
    }
}
