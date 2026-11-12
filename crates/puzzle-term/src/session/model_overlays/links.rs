//! Link identity is resolved against presented text, never against a future drawable.
//!
//! The Session owns detection and opening policy. The model owns interaction
//! timing, and the render view receives a sealed list of visible source cells.
//! Row versions prevent output arriving after presentation from changing a
//! modifier-click's target. Suppressing an obsolete hover is safer than opening
//! an unseen replacement; the next presented table restores normal lookup.
//! URL detection visits only the hovered logical line. Explicit group matching
//! is restricted to visible rows and never scans retained scrollback.
//!
//! Queries are source-driven, not per-cell parse callbacks. Pointer movement or
//! a changed presented row can invalidate one query; an unchanged frame skips it.
//! Probe keeps both the queried cell and exact current row versions, including
//! missing rows. Missing and replaced rows must invalidate grouped link ink.
//! A pending future snapshot never becomes the input geometry for these checks.
//! Query scratch owns its URI, visible cells and logical-line byte mapping.
//! Taking that scratch temporarily lets VT be borrowed without shared mutation.
//! Returning it after resolution keeps the warmed allocation high-water marks.
//! Unsupported URI schemes still appear as readable data but are not openable.
//! The host OS table supplies the primary modifier independently of rendering.
//! Both direct OpenLink commands and gestures repeat policy after interceptors.
//! Configuration changes invalidate only this query, not glyph/atlas resources.
//! Empty pointer/source combinations avoid detection and scratch work entirely.
//! Group matching checks every generation-bearing OSC handle before use.
//! Visible target cells include wide spacers but never duplicate UTF-8 bytes.
//! The view receives logical cell positions, leaving pixel conversion to geometry.
//! No timer or font lookup happens while resolving or validating the URI.
//! The provider owns the one tooltip deadline after a new target appears.
use super::*;
use puzzle_overlay::geometry::Cell;
/// The fixed scheme filter applies even after a command is rewritten.
pub(in crate::session) mod policy;

impl Session {
    /// Registers the public passive hover model and its independent drawing half.
    pub(super) fn install_link_overlay(&mut self) {
        self.overlays_mut().add(
            Box::<puzzle_overlay::builtins::links::Links>::default(),
            Box::new(puzzle_overlay::builtins::links::LinksView),
        );
    }
    /// Refreshes the hovered target from current data only where presented versions agree.
    pub(in crate::session) fn refresh_link(&mut self) {
        if !self.model_overlays.installed {
            return;
        }
        // The outer option means source unchanged; its inner option can mean no target.
        let Some(cell) = self.link_query() else {
            return;
        };
        // Temporarily own the warmed query buffers while borrowing live VT read-only.
        let mut probe = std::mem::take(&mut self.model_overlays.link_probe);
        let geometry = &self.overlay_source.geometry;
        let view = self.buffer();
        // Clear a previous URI even when the new cell has no target.
        probe.target.uri.clear();
        probe.target.cells.clear();
        if let Some(cell) = cell
            && let (Some(first), Some(last)) = (geometry.lines.first(), geometry.lines.last())
        {
            view.link_target_into(
                cell.line,
                cell.column,
                first.id..=last.id,
                self.model_overlays.link_policy.detect,
                &mut probe.target,
            );
        }
        // Grouped cells are actionable only when their current rows match presented pixels.
        probe.target.cells.retain(|(line, _)| {
            geometry.lines.iter().any(|presented| {
                presented.id == *line
                    && view
                        .line(*line)
                        .is_some_and(|row| row.version() == presented.version)
            })
        });
        // Cache the exact query even on negative results to avoid repeated scanning.
        probe.cell = cell;
        // Capacity survives source invalidation and changing row versions.
        probe.rows.clear();
        probe.rows.extend(
            geometry
                .lines
                .iter()
                .map(|line| (line.id, view.line(line.id).map(|row| row.version()))),
        );
        self.commit_link_probe(probe);
    }
    /// Replaces annotation data after lookup, preserving each owned buffer's capacity.
    fn commit_link_probe(&mut self, probe: Probe) {
        let state = &mut self.model_overlays;
        let uri = &probe.target.uri;
        // Compare source coordinates before overwriting the previous annotation.
        let changed = state.link.uri != *uri
            || !state.link.cells.iter().copied().eq(probe
                .target
                .cells
                .iter()
                .map(|&(line, column)| Cell { line, column }));
        // Frozen source strings retain capacity independently of query scratch.
        state.link.uri.clear();
        state.link.uri.push_str(uri);
        state.link.cells.clear();
        state.link.cells.extend(
            probe
                .target
                .cells
                .iter()
                .map(|&(line, column)| Cell { line, column }),
        );
        state.link.primary = self.keybindings.platform.primary();
        state.link.openable = state.link_policy.allows(uri, !self.keybindings.web);
        // Restore ownership before notifying the model of a changed hover source.
        state.link_probe = probe;
        if changed {
            self.overlay_changed(EventKinds::POINTER);
        }
    }
    /// Unchanged sources skip URL detection; None differs from a changed empty target.
    fn link_query(&self) -> Option<Option<Cell>> {
        // Presented bounds exclude stale input that falls outside the current grid.
        let geometry = &self.overlay_source.geometry;
        let view = self.buffer();
        let cell = self
            .overlays
            .pointer
            .filter(|point| geometry.grid.contains(*point))
            .and_then(|point| geometry.hit_test(point).cell)
            .filter(|cell| {
                geometry.lines.iter().any(|line| {
                    line.id == cell.line
                        && view
                            .line(cell.line)
                            .is_some_and(|row| row.version() == line.version)
                })
            });
        // An absent target has no tooltip or opener state to clear again.
        if cell.is_none() && self.model_overlays.link.uri.is_empty() {
            return None;
        }
        // Exact source/version identity avoids repeated URL scans on unchanged frames.
        let probe = &self.model_overlays.link_probe;
        let current_rows = || {
            geometry
                .lines
                .iter()
                .map(|line| (line.id, view.line(line.id).map(|row| row.version())))
        };
        if probe.cell == cell && probe.rows.iter().copied().eq(current_rows()) {
            return None;
        }
        Some(cell)
    }
}

/// Exact cache identity, updated only when the source or presented cells change.
#[derive(Default)]
pub(super) struct Probe {
    /// Missing or obsolete presented pixels cannot produce an opening target.
    cell: Option<Cell>,
    /// URI, visible cells and logical-line scratch reuse the query high-water mark.
    target: crate::vt::view::LinkTarget,
    /// Both missing and replaced source rows invalidate grouped-link lookup.
    rows: Vec<(crate::vt::tracked::LineId, Option<u64>)>,
}

/// Settings invalidate the cold query, never glyph or atlas contents.
impl Session {
    /// A changed host policy forces lookup even when the presented cells match.
    pub(in crate::session) fn invalidate_link_hover(&mut self) {
        self.model_overlays.link_probe.cell = None;
        self.model_overlays.link_probe.rows.clear();
        self.overlay_changed(EventKinds::POINTER);
    }
    /// Applies validated configuration before the next hover or API request.
    pub(in crate::session) fn apply_link_settings(&mut self, settings: &crate::settings::Settings) {
        let policy = &mut self.model_overlays.link_policy;
        policy.detect = settings.links_detect_urls;
        policy.enabled = settings.links_open != "disabled";
        policy.schemes.clone_from(&settings.links_schemes);
        self.invalidate_link_hover();
    }
}
