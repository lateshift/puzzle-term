//! Session-timestamped OSC 133 boundaries feed immutable command-block sources.
//!
//! VT owns tracked positions and monotonic command IDs, never a clock. Session
//! records coarse transition times from the feed's injected clock. A bounded
//! timing table follows the bounded mark list; reflow changes only VT positions.
//! Source capture walks marks once and emits only blocks crossing visible rows.
//! The view never extracts output or reads history; copy is an explicit command.
//!
//! Execution begins at C, not at A: time spent typing a command is not runtime.
//! D closes the duration with the same injected clock used by output parsing.
//! Keeping times outside VT preserves deterministic, clock-free terminal replay.
//! The timing map is keyed by VT-assigned IDs; application text cannot alias it.
//! A new prompt also closes an incomplete visible output interval, but cannot
//! manufacture a completion status or a duration for an interrupted command.
//! Reflow changes the tracked mark locations without changing command identity.
//! Eviction removes the marks through BufferView and eventually trims the clocks.
//! There is no per-frame timer for running commands: only final durations display.
//! Source capture clips to the viewport before crossing the snapshot channel.
//! Thus the render thread never copies retained output just to draw a gutter.
//! A copy button sends the command ID back to the owner; extraction happens there.
//! The model source uses retained capacity, and absent zones produce no blocks.
//! Terminal reset preserves monotonic IDs, so old buttons cannot name new output.
//! Finishing at column zero excludes the next prompt row from the colored gutter.
//! The exact C/D positions remain unchanged for half-open clipboard extraction.
use super::*;
use crate::vt::{
    tracked::LineId,
    zones::{ZoneKind, ZoneMark},
};
use puzzle_overlay::models::blocks::Block;
use std::{collections::BTreeMap, time::Duration};

/// Timing belongs to a command identity, independently of its remapped rows.
#[derive(Default)]
pub(in crate::session) struct Blocks {
    /// At most one timing record per retained OSC command identity.
    times: BTreeMap<u64, Timing>,
    /// Stable model registration receives explicit post-policy copy receipts.
    provider: Option<puzzle_overlay::registry::ProviderId>,
}
/// Input and prompt printing are excluded from command execution duration.
struct Timing {
    /// Injected owner time when OSC C was parsed.
    start: Instant,
    /// None while the program still owns the command's output interval.
    duration: Option<Duration>,
}
/// Coarse parser facts update only bounded execution records.
impl Blocks {
    /// Takes one parser fact without consulting a platform clock.
    pub(in crate::session) fn boundary(&mut self, id: u64, kind: ZoneKind, now: Instant) {
        match kind {
            // C replaces only this command's clock; earlier commands retain their durations.
            ZoneKind::Output => {
                // One allocation per new command is cold metadata, not per-byte parsing.
                self.times.insert(
                    id,
                    Timing {
                        start: now,
                        duration: None,
                    },
                );
            }
            // D without a retained C is incomplete metadata, not a new running command.
            ZoneKind::Finished => {
                if let Some(timing) = self.times.get_mut(&id) {
                    // Saturation also makes deliberately rewound diagnostic clocks harmless.
                    timing.duration = Some(now.saturating_duration_since(timing.start));
                }
            }
            // Prompt and input boundaries do not measure execution.
            _ => {}
        }
    }
    /// Old command clocks disappear together with their bounded tracked marks.
    pub(in crate::session) fn trim(&mut self, oldest: Option<u64>) {
        // The oldest retained mark gives a monotonic lower bound on useful clocks.
        self.times
            .retain(|id, _| oldest.is_some_and(|first| *id >= first));
    }
}
/// The Session publishes values, leaving interaction and ink to the public provider.
impl Session {
    /// Built-ins register through the same split model/view API offered to hosts.
    pub(super) fn install_block_overlay(&mut self) {
        self.model_overlays.blocks.provider = Some(self.overlays_mut().add(
            Box::<puzzle_overlay::builtins::blocks::Blocks>::default(),
            Box::<puzzle_overlay::builtins::blocks::BlocksView>::default(),
        ));
    }
    /// Policy and extraction finish before the model can display success.
    pub(in crate::session) fn copy_block_output(&mut self, id: u64, now: Instant) {
        // Earlier queued work must not supply the receipt for this gesture.
        self.pump(now);
        self.submit(
            crate::messages::Command::Copy(crate::selection::CopyTarget::ZoneOutput(id)),
            crate::Origin::User,
        );
        let copied = self.drain_commands(now);
        if let (Some(provider), Some(epoch)) =
            (self.model_overlays.blocks.provider, self.overlay_epoch)
        {
            // Publish only identity/outcome/time; copied text stays out of view snapshots.
            let receipt = puzzle_overlay::builtins::blocks::CopyFeedback {
                id,
                copied,
                started: now.saturating_duration_since(epoch).as_secs_f64(),
            };
            self.overlays
                .replace_state(provider, std::sync::Arc::new(receipt));
            // The next model update schedules only the finite flash/removal deadlines.
            self.needs_frame = true;
            self.services.waker.request_frame();
        }
    }
    /// One pass pairs boundaries; a new prompt cannot complete an older output.
    pub(super) fn copy_blocks(&self, out: &mut Models) {
        // Reuse the publication's block buffer across rotating snapshot slots (P1).
        out.blocks.clear();
        // BufferView has already filtered evicted tracked positions.
        let view = self.buffer();
        let Some(ids) = view.line_ids() else {
            return;
        };
        // The viewport starts in retained row order, independently of screen row numbers.
        let first = self
            .terminal
            .screen()
            .ring
            .retained(self.viewport.top_index())
            .map(|row| row.id);
        let Some(first) = first else {
            return;
        };
        // Fractional scrolling may include an extra edge row.
        let count = self.viewport.visible_count(self.terminal.size().1);
        // Clamp the end to actual retained rows, including a shortened bottom edge.
        let last = self
            .terminal
            .screen()
            .ring
            .retained(self.viewport.top_index() + count.saturating_sub(1))
            .map_or(first, |row| row.id);
        // Pair marks once; a later C supersedes an unfinished prior C.
        let mut pending: Option<&ZoneMark> = None;
        for mark in view.zones() {
            match mark.kind {
                ZoneKind::Output => pending = Some(mark),
                // D without a retained C is incomplete metadata, not a new running command.
                ZoneKind::Finished => {
                    // Identity pairing prevents an unrelated D from completing older output.
                    if let Some(start) = pending.take().filter(|start| start.id == mark.id) {
                        let end = block_end(&self.terminal, start, mark);
                        self.visible_block(out, start, end, mark.exit, first..=last);
                    }
                }
                ZoneKind::Prompt => {
                    // An interrupted command ends at the next prompt without an exit code.
                    if let Some(start) = pending.take() {
                        self.visible_block(
                            out,
                            start,
                            block_end(&self.terminal, start, mark),
                            None,
                            first..=last,
                        );
                    }
                }
                // Prompt and input boundaries do not measure execution.
                _ => {}
            }
        }
        // Running output extends to the last retained row with no completion badge.
        if let Some(start) = pending {
            self.visible_block(out, start, *ids.end(), None, first..=last);
        }
    }
    /// Only the clipped interval and one duration cross the publication seam.
    fn visible_block(
        &self,
        out: &mut Models,
        start: &ZoneMark,
        end: LineId,
        exit: Option<i32>,
        visible: std::ops::RangeInclusive<LineId>,
    ) {
        // The sealed source carries only the part intersecting this viewport.
        let (first, last) = (*visible.start(), *visible.end());
        if end < first || start.line > last {
            return;
        }
        // Copy a scalar duration, never a platform clock handle or mutable record.
        let duration = self
            .model_overlays
            .blocks
            .times
            .get(&start.id)
            .and_then(|time| time.duration)
            .map(|duration| duration.as_secs_f64());
        // Theme color is chosen later by the view from the optional exit status.
        out.blocks.push(Block {
            id: start.id,
            first: start.line.max(first),
            last: end.min(last),
            exit,
            duration,
        });
    }
}

/// Column-zero completion excludes the following prompt's row from the gutter.
fn block_end(term: &crate::vt::Terminal, start: &ZoneMark, end: &ZoneMark) -> LineId {
    if end.column == 0 && end.line > start.line {
        // Retained row order is authoritative, including reflowed monotonic identities.
        // Locate the predecessor by a row handle, never by scanning output cells.
        let ring = &term.screen().ring;
        ring.index_of(end.line)
            .and_then(|index| index.checked_sub(1))
            .and_then(|index| ring.retained(index))
            .map_or(start.line, |row| row.id)
    } else {
        end.line
    }
}
