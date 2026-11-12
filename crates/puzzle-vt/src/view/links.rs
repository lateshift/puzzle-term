//! Hyperlinks from OSC 8 and on-demand logical-line URL detection (§6.11).
//!
//! OSC 8 stores a generation-checked handle in sparse cell metadata.
//! Plain text has no handle, so only a caller's requested logical line is
//! scanned. Nothing in the parser's per-cell path runs URL detection.
//! Detection returns data, never opens a URL: modifier policy and allowed
//! schemes belong to the host. ASCII delimiters and trailing punctuation
//! are trimmed without a regex or a whole-buffer scan.
//!
//! A physical row is not a safe search unit: an application may print
//! `https://exa` at the margin and continue `mple.org` after autowrap.
//! `line_text` follows the WRAPPED flag and records a cell origin for each
//! UTF-8 byte. A multibyte path segment or combining cluster still maps
//! back to its one owning grid cell.
//!
//! Stored OSC 8 metadata wins over inferred text because the application
//! explicitly supplied that target. Neither kind is opened here. A host
//! still requires its link gesture and applies scheme policy, especially
//! before opening a local `file:` URL.
use super::*;
/// Group-aware cold hover resolution, separate from ordinary link queries.
mod target;
use crate::links::LinkId;
/// Visible link cells and their target URI.
pub use target::LinkTarget;

/// One detected URL with its source cells and owned target text.
struct Detected {
    /// Text begins at this retained line and cell.
    start: (LineId, usize),
    /// Every source byte maps to its owning cell, including UTF-8 bytes.
    cells: Vec<(LineId, usize)>,
    /// Target string after trimming punctuation at the right edge.
    uri: String,
}

impl BufferView<'_> {
    /// Resolves a live OSC 8 handle or detects a URL under one cell.
    ///
    /// The owned return keeps the API valid for both stored and computed
    /// targets. Host link-opening policy still checks the scheme (§16).
    #[must_use]
    pub fn link_at(self, line: LineId, column: usize) -> Option<String> {
        // A generation-checked explicit target keeps its authority even
        // when the displayed characters resemble a different plain URL.
        if let Some(uri) = explicit(self, line, column) {
            return Some(uri.to_string());
        }
        // Plain text is cold: scan only the queried logical line, not
        // every byte passing through the terminal's print path.
        detected(self, line)
            .into_iter()
            .find(|hit| hit.cells.contains(&(line, column)))
            .map(|hit| hit.uri)
    }

    /// Enumerates explicit and detected links overlapping retained lines.
    ///
    /// A logical line is scanned once even when its URL crosses a wrap.
    /// The iterator owns cold-query results so it need not retain scratch.
    pub fn links(
        self,
        range: RangeInclusive<LineId>,
    ) -> impl Iterator<Item = (LineId, usize, String)> {
        // Owning this query result lets callers iterate without retaining
        // the temporary byte-to-cell map built during URL detection.
        let mut output = Vec::new();
        let mut last_group = None;
        for line in self.lines(range.clone()) {
            for extra in &line.row.extras {
                if let Some(uri) = explicit(self, line.id(), extra.column) {
                    output.push((line.id(), extra.column, uri.to_string()));
                }
            }
            // Consecutive physical rows of one wrapped line would repeat
            // the same URL unless its logical start is remembered.
            let Some(group) = self.logical_line(line.id()) else {
                continue;
            };
            if last_group == Some(*group.start()) {
                continue;
            }
            last_group = Some(*group.start());
            for hit in detected(self, line.id()) {
                // OSC 8 markup takes precedence when both mechanisms cover
                // the same cell, so one click has one unambiguous target.
                // A URL can start before the range and continue into it;
                // any overlapping source cell makes the target relevant.
                if hit.cells.iter().any(|pos| range.contains(&pos.0))
                    && explicit(self, hit.start.0, hit.start.1).is_none()
                {
                    output.push((hit.start.0, hit.start.1, hit.uri));
                }
            }
        }
        output.into_iter()
    }
}

/// Resolves a sparse link only after checking its table generation.
fn explicit(view: BufferView<'_>, line: LineId, column: usize) -> Option<&str> {
    // `LinkId` embeds a generation. A stale cell handle cannot return
    // the unrelated URI of a recycled link-table slot.
    let row = view.line(line)?.row;
    let raw = row
        .extras
        .iter()
        .find(|extra| extra.column == column)?
        .link_id?;
    view.term.links.resolve(LinkId(raw))
}

/// Builds byte and cell maps for one soft-wrapped logical line.
fn line_text(view: BufferView<'_>, id: LineId) -> Option<(String, Vec<(LineId, usize)>)> {
    let group = view.logical_line(id)?;
    let mut text = String::new();
    let mut cells = Vec::new();
    for line in view.lines(group) {
        for column in 0..line.cells().len() {
            let cell = line.cell(column)?;
            // The wide spacer owns no independent bytes; its lead already
            // contributed the glyph's UTF-8 text and source cell.
            if cell.width == 0 {
                continue;
            }
            let part = match cell.text {
                CellText::Empty => " ".to_string(),
                CellText::Scalar(ch) => ch.to_string(),
                CellText::Cluster(cluster) => cluster.to_string(),
            };
            // The scanner uses byte offsets. Repeating the same origin
            // for each UTF-8 byte avoids mistaking a byte for a column.
            cells.extend(std::iter::repeat_n((line.id(), column), part.len()));
            text.push_str(&part);
        }
        if !line.wrapped() {
            break;
        }
    }
    Some((text, cells))
}

/// Finds supported URL prefixes only at an ASCII token boundary.
fn scheme_at(bytes: &[u8], index: usize) -> bool {
    let tail = &bytes[index..];
    let prefix = [b"https://".as_slice(), b"http://", b"mailto:", b"file://"];
    // Embedded text such as foohttps:// should not become a clickable
    // suffix, while punctuation before a URL may introduce one in prose.
    let boundary = index == 0 || !bytes[index - 1].is_ascii_alphanumeric();
    boundary && prefix.iter().any(|candidate| tail.starts_with(candidate))
}

/// Scans one logical line and maps each URL back to its cell interval.
fn detected(view: BufferView<'_>, id: LineId) -> Vec<Detected> {
    let Some((text, positions)) = line_text(view, id) else {
        return Vec::new();
    };
    tokens(text.as_bytes())
        .filter_map(|(start, end)| {
            let &origin = positions.get(start)?;
            Some(Detected {
                start: origin,
                cells: positions[start..end].to_vec(),
                uri: text[start..end].to_string(),
            })
        })
        .collect()
}

/// One allocation-free token policy serves cold contents queries and warmed hover.
fn tokens(bytes: &[u8]) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut index = 0;
    std::iter::from_fn(move || {
        while index < bytes.len() {
            // Prefix recognition requires a token boundary, excluding embedded suffixes.
            if !scheme_at(bytes, index) {
                index += 1;
                continue;
            }
            let start = index;
            // ASCII delimiters leave every non-ASCII UTF-8 path byte unchanged.
            while index < bytes.len()
                && !matches!(
                    bytes[index],
                    b' ' | b'\t' | b'\r' | b'\n' | b'<' | b'>' | b'"' | b'\''
                )
            {
                index += 1;
            }
            let mut end = index;
            // Sentence punctuation follows the pre-existing contents API contract.
            while end > start
                && matches!(
                    bytes[end - 1],
                    b'.' | b',' | b';' | b':' | b'!' | b'?' | b')' | b']' | b'}'
                )
            {
                end -= 1;
            }
            if end > start {
                return Some((start, end));
            }
        }
        None
    })
}

#[cfg(test)]
#[path = "links/tests.rs"]
mod tests;
