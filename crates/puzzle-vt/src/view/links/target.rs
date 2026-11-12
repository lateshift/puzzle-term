//! Cold hovered-link queries preserve explicit grouping and detected URL cells.
//!
//! Only the caller's visible rows are considered for OSC 8 group highlighting.
//! Detection shares the contents API's token scanner, preserving one set
//! of punctuation and UTF-8 mapping rules. Wide spacers join their leading cell.
//! This is a cold explicit pointer query, not a parser hook or a background scan.
//! The caller supplies a visible interval to bound grouped underline storage.
//! A single explicit target can have several disjoint spans and several URIs;
//! clicking one span opens that span's URI while highlighting its whole group.
//! Every stored handle is generation checked before grouping, so eviction cannot
//! turn old cell metadata into a new target. Unsupported schemes remain data:
//! the Session, after interceptors, decides whether navigation is permitted.
//! Detected targets preserve the logical-line scanner's punctuation rules.
//! Scalar-to-byte mapping can repeat a position; deduplication keeps one dot run
//! per occupied cell. Wide spacers have no bytes and are explicitly appended.
//!
//! Two public entry points serve different ownership needs. The cold contents
//! query returns an owned result, while hover accepts caller-owned reusable scratch.
//! Scratch survives both hits and misses, so continuous output under a pointer
//! does not allocate again after the observed logical-line high-water mark.
//! Scalar text is encoded into a four-byte stack buffer, never a temporary String.
//! Cluster text is borrowed from row extras and copied directly into that scratch.
//! UTF-8 byte offsets retain their original leading-cell coordinates for lookup.
//! The scanner strips only ASCII delimiters, leaving non-ASCII paths intact.
//! A detected URI can extend beyond the visible rows while only its visible
//! cells receive regions. Grouped OSC links similarly return no hidden cells.
//! The caller's visibility interval affects annotation, never URI interpretation.
//! Explicit handles are resolved before plain detection, even when detection is off.
//! Queries do not change terminal cells, tracked markers or link-table contents.
use super::*;

/// One target and the visible grid cells that should share its hover underline.
#[derive(Default)]
pub struct LinkTarget {
    /// Bounded URI data; callers still apply gesture and opening policies.
    pub uri: String,
    /// Sorted source positions, including wide-character spacer cells.
    pub cells: Vec<(LineId, usize)>,
    /// UTF-8 logical-line scratch survives repeated hover queries.
    text: String,
    /// Byte offsets map to cells without allocating per scalar.
    positions: Vec<(LineId, usize)>,
}
/// Pointer lookup borrows current rows and returns an owned, bounded target.
impl BufferView<'_> {
    /// Resolves OSC grouping or one on-demand URL without scanning hidden history.
    pub fn link_target(
        self,
        line: LineId,
        column: usize,
        visible: RangeInclusive<LineId>,
        detect_urls: bool,
    ) -> Option<LinkTarget> {
        // Cold callers choose ownership; hot hover callers retain this value themselves.
        let mut target = LinkTarget::default();
        self.link_target_into(line, column, visible, detect_urls, &mut target)
            .then_some(target)
    }

    /// Reuses all scratch for source-driven hover; no warmed query allocation is needed.
    pub fn link_target_into(
        self,
        line: LineId,
        column: usize,
        visible: RangeInclusive<LineId>,
        detect_urls: bool,
        target: &mut LinkTarget,
    ) -> bool {
        // Clear lengths, not allocations, before an absent or replaced target is resolved.
        target.uri.clear();
        target.cells.clear();
        // Missing rows or targets propagate as false without replacing the scratch object.
        self.resolve_target(line, column, visible, detect_urls, target)
            .is_some()
    }

    /// Option-based lookup propagates missing rows without constructing an owned target.
    fn resolve_target(
        self,
        line: LineId,
        column: usize,
        visible: RangeInclusive<LineId>,
        detect_urls: bool,
        target: &mut LinkTarget,
    ) -> Option<()> {
        // A spacer inherits its leader's metadata rather than resolving independently.
        let cell = self.line(line)?.cell(column)?;
        // A wide spacer is visually part of the link drawn by its preceding cell.
        let column = if cell.width == 0 {
            column.saturating_sub(1)
        } else {
            column
        };
        // Explicit metadata wins over URL-looking text in the same cell.
        if let Some(raw) = self
            .line(line)?
            .row
            .extras
            .iter()
            .find(|extra| extra.column == column)
            .and_then(|extra| extra.link_id)
            && let Some(uri) = self.term.links.resolve(crate::links::LinkId(raw))
        {
            // The hovered segment chooses the opening URI, not the first grouped span.
            target.uri.push_str(uri);
            self.group_cells(raw, visible, target);
            return Some(());
        }
        // Disabling detection never disables an application-authored OSC target.
        if !detect_urls {
            return None;
        }
        self.detected_target(line, column, visible, target)
    }

    /// Resolves plain URL text and expands its byte span into visible occupied cells.
    fn detected_target(
        self,
        line: LineId,
        column: usize,
        visible: RangeInclusive<LineId>,
        target: &mut LinkTarget,
    ) -> Option<()> {
        // The byte/cell map is retained between source changes, including no-URL results.
        fill_text(self, line, &mut target.text, &mut target.positions)?;
        // Token bounds come from the same scanner as BufferView::link_at.
        let (start, end) = find_target(target.text.as_bytes(), &target.positions, (line, column))?;
        // Preserve the complete URI even when some of its cells are offscreen.
        target.uri.push_str(&target.text[start..end]);
        for pos in target.positions[start..end]
            .iter()
            .copied()
            .filter(|pos| visible.contains(&pos.0))
        {
            // UTF-8 bytes can share an owning cell; emit its underline only once.
            // All bytes of one grapheme share an origin, including combining sequences.
            if target.cells.last() != Some(&pos) {
                target.cells.push(pos);
            }
        }
        // Spacers own no bytes, but share the wide leading cell's visible annotation.
        let mut index = 0;
        while index < target.cells.len() {
            let (line, column) = target.cells[index];
            if self.line(line)?.cell(column)?.width == 2 {
                target.cells.insert(index + 1, (line, column + 1));
                // Skip the inserted spacer rather than interpreting it as another leader.
                index += 1;
            }
            index += 1;
        }
        Some(())
    }
    /// Visits sparse link metadata only on visible rows, retaining target capacity.
    fn group_cells(self, raw: u32, visible: RangeInclusive<LineId>, target: &mut LinkTarget) {
        for row in self.lines(visible) {
            // Only sparse explicit metadata is visited, never every cell of history.
            for extra in &row.row.extras {
                if extra.link_id.is_some_and(|other| {
                    self.term
                        .links
                        .same_group(crate::links::LinkId(raw), crate::links::LinkId(other))
                }) {
                    // Extras identify leading cells; spacers inherit their underline.
                    target.cells.push((row.id(), extra.column));
                    if row.cell(extra.column).is_some_and(|cell| cell.width == 2) {
                        target.cells.push((row.id(), extra.column + 1));
                    }
                }
            }
        }
    }
}

/// Appends a logical line to caller-owned UTF-8 scratch without temporary scalar strings.
fn fill_text(
    view: BufferView<'_>,
    id: LineId,
    text: &mut String,
    positions: &mut Vec<(LineId, usize)>,
) -> Option<()> {
    // Both buffers preserve capacity across a different token or a negative result.
    text.clear();
    positions.clear();
    // Soft-wrap metadata identifies the only logical line permitted for detection.
    let group = view.logical_line(id)?;
    let ring = &view.term.screen().ring;
    let first = ring.index_of(*group.start())?;
    let last = ring.index_of(*group.end())?;
    // Walk only this logical line's retained rows, never the rest of scrollback.
    for index in first..=last {
        let row = ring.retained(index)?;
        // LineView supplies scalar/cluster semantics without serializing the whole row.
        let line = crate::view::LineView::new(row, view.term);
        for column in 0..row.cells.len() {
            let cell = line.cell(column)?;
            // Wide spacers contribute geometry later, but never independent UTF-8 bytes.
            if cell.width == 0 {
                continue;
            }
            // UTF-8 needs at most four bytes for one Unicode scalar.
            let mut scalar = [0; 4];
            let part = match cell.text {
                CellText::Empty => " ",
                CellText::Scalar(ch) => ch.encode_utf8(&mut scalar),
                CellText::Cluster(cluster) => cluster,
            };
            // Every byte can be traced back to its owning cell without a hash map.
            positions.extend(std::iter::repeat_n((row.id, column), part.len()));
            text.push_str(part);
        }
    }
    Some(())
}

/// Finds one matching token using the existing scheme and punctuation policy.
fn find_target(
    bytes: &[u8],
    positions: &[(LineId, usize)],
    cell: (LineId, usize),
) -> Option<(usize, usize)> {
    super::tokens(bytes).find(|&(start, end)| positions[start..end].contains(&cell))
}
