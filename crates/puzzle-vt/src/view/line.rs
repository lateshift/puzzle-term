//! Borrowed rows and cells with scalar/cluster text (§6.13).
//!
//! `Cell::CLUSTER` means the printable grapheme lives in a sorted sparse
//! extra, while `WIDE_SPACER` is a second grid column without independent
//! text. Exposing those cases explicitly prevents selection, search and
//! the JS bulk API from duplicating a wide glyph or losing combining marks.
//!
//! Colors stay tagged here. Theme and OSC palette changes should recolor
//! an existing cell without rewriting the row or invalidating a selection.
//! A caller needing RGB can resolve the tag against the current palette.
//!
//! MULTICELL shares the existing special-layout dispatch with wide spacers.
//! Its word is a private slot index, including valid index zero, so it must be
//! resolved before the ordinary unwritten/scalar cases. Only the first retained
//! fragment's first column exposes the payload. Other covered cells have width
//! zero, preventing generic copy/search clients from duplicating a heading.
//! Geometry-aware readers use the sparse block accessor to recover the whole
//! rectangle, including original offsets when history has evicted its root.
use super::*;
use crate::links::LinkId;

/// A cell's printable text without allocating a temporary string.
///
/// A cluster borrow points into `Row::extras`, so it lives only as long as
/// the buffer view and cannot outlive terminal mutation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CellText<'a> {
    /// The cell has never received a printable scalar.
    Empty,
    /// One decoded Unicode scalar is stored inline.
    Scalar(char),
    /// A longer grapheme cluster belongs to a sparse row extra.
    Cluster(&'a str),
}

/// One borrowed cell and its semantic width and link metadata.
///
/// Width zero identifies only a spacer, never a combining mark; marks
/// already joined the preceding cell's cluster during parsing.
#[derive(Clone, Copy, Debug)]
pub struct CellView<'a> {
    /// Original compact cell data, including unresolved tagged colors.
    pub raw: Cell,
    /// Scalar, cluster or empty cell text.
    pub text: CellText<'a>,
    /// Physical span (normally one or two); wide/block continuations have width zero.
    pub width: u8,
    /// Sparse underline override, if present.
    pub underline_color: Option<PackedColor>,
    /// Generation-checked link target, if still live.
    ///
    /// A stale numeric handle resolves to `None` after table eviction.
    pub link: Option<&'a str>,
    /// One authoritative colour state, borrowed rather than copied per cell.
    colors: &'a crate::color_table::ColorTable,
}

impl CellView<'_> {
    /// Returns the inline SGR attributes, including underline style.
    #[must_use]
    pub fn attrs(self) -> Attrs {
        self.raw.attrs
    }

    /// Keeps the tagged foreground for theme-time color resolution.
    #[must_use]
    pub fn foreground(self) -> PackedColor {
        self.raw.fg
    }

    /// Keeps the tagged background for palette-time color resolution.
    #[must_use]
    pub fn background(self) -> PackedColor {
        self.raw.bg
    }

    /// Resolves the foreground using current OSC overrides and host theme.
    #[must_use]
    pub fn resolved_foreground(
        self,
        theme_default: [u8; 3],
        theme_palette: &[[u8; 3]; 256],
    ) -> [u8; 3] {
        // This contents API accepts an explicit host theme, including before a
        // Session installs one. Only application overrides supersede that argument.
        resolve(
            self.raw.fg,
            self.colors
                .override_value(crate::color_table::ColorSlot::Dynamic(
                    crate::color_table::DynamicColor::Foreground,
                ))
                .and_then(crate::color_table::ColorValue::rgb)
                .unwrap_or(theme_default),
            self.colors,
            theme_palette,
        )
    }

    /// Resolves the background using current OSC overrides and host theme.
    #[must_use]
    pub fn resolved_background(
        self,
        theme_default: [u8; 3],
        theme_palette: &[[u8; 3]; 256],
    ) -> [u8; 3] {
        // Keep the same precedence for paper; the renderer instead receives a
        // sealed table whose defaults already include its sampled theme.
        resolve(
            self.raw.bg,
            self.colors
                .override_value(crate::color_table::ColorSlot::Dynamic(
                    crate::color_table::DynamicColor::Background,
                ))
                .and_then(crate::color_table::ColorValue::rgb)
                .unwrap_or(theme_default),
            self.colors,
            theme_palette,
        )
    }
}

/// Decodes tagged default, indexed and RGB colors without mutating a cell.
fn resolve(
    color: PackedColor,
    default: [u8; 3],
    colors: &crate::color_table::ColorTable,
    palette: &[[u8; 3]; 256],
) -> [u8; 3] {
    // Palette indices stay tagged in the grid so an OSC 4 change can
    // recolor old cells without touching their content versions.
    match color.0 >> 24 {
        1 => {
            let index = color.0 as u8 as usize;
            colors
                .override_value(crate::color_table::ColorSlot::Palette(index as u8))
                .and_then(crate::color_table::ColorValue::rgb)
                .unwrap_or(palette[index])
        }
        2 => [(color.0 >> 16) as u8, (color.0 >> 8) as u8, color.0 as u8],
        // Unknown tags are treated as default; malformed external data
        // must not index beyond the palette or panic (§16, R8).
        _ => default,
    }
}

/// One retained row, still owned by its terminal screen.
///
/// ID survives scroll, while version changes after in-place edits; both
/// are needed to tell observers whether their content remains valid.
#[derive(Clone, Copy)]
pub struct LineView<'a> {
    /// Row identity, cells, sparse extras and version.
    pub(super) row: &'a Row,
    /// Needed only to validate link generations and palette overrides.
    pub(super) term: &'a Terminal,
}

impl<'a> LineView<'a> {
    /// Pairs a retained row with its source terminal for link resolution.
    pub(super) fn new(row: &'a Row, term: &'a Terminal) -> Self {
        Self { row, term }
    }

    /// Stable identity survives scrolling until ring eviction.
    #[must_use]
    pub fn id(self) -> LineId {
        self.row.id
    }

    /// A soft-wrapped row continues into the next retained row.
    #[must_use]
    pub fn wrapped(self) -> bool {
        self.row.flags.contains(RowFlags::WRAPPED)
    }

    /// Content version invalidates a selection after an in-place rewrite.
    #[must_use]
    pub fn version(self) -> u64 {
        self.row.version
    }

    /// Prompt marks are row metadata, independent of OSC zone details.
    #[must_use]
    pub fn prompt(self) -> bool {
        self.row.flags.contains(RowFlags::PROMPT)
    }

    /// Borrows logical full-width cells, including the row's implicit BCE suffix.
    #[must_use]
    pub fn cells(self) -> &'a crate::row::RowCells {
        &self.row.cells
    }

    /// Resolves a column, skipping no data until the caller asks for text.
    ///
    /// The fixed cell array is the common case; sparse extra lookup is
    /// needed only to recover cluster, link or underline metadata.
    #[must_use]
    pub fn cell(self, column: usize) -> Option<CellView<'a>> {
        // Bounds checking includes implicit erased cells in a lazily cleared row.
        let raw = *self.row.cells.get(column)?;
        let extra = self.row.extras.iter().find(|item| item.column == column);
        let (text, width) = self.cell_text(raw, column, extra);
        let link = extra
            .and_then(|item| item.link_id)
            .and_then(|id| self.term.links.resolve(LinkId(id)));
        Some(CellView {
            raw,
            text,
            width,
            underline_color: extra.and_then(|item| item.underline_color),
            link,
            colors: &self.term.colors,
        })
    }
    /// Decodes the existing special-layout branch without an ordinary per-cell probe.
    fn cell_text(
        self,
        raw: Cell,
        column: usize,
        extra: Option<&'a crate::CellExtra>,
    ) -> (CellText<'a>, u8) {
        if raw
            .flags
            .intersects(CellFlags::WIDE_SPACER | CellFlags::MULTICELL)
        {
            if raw.flags.contains(CellFlags::MULTICELL)
                && let Some(block) = self.multicell(column)
                && block.column == column
                && block
                    .fragments
                    .first()
                    .is_some_and(|fragment| fragment.line == self.id())
            {
                // The first retained fragment owns text, including a clipped root.
                return (CellText::Cluster(&block.text), block.columns as u8);
            }
            // An invalid record and a legitimate continuation both expose no
            // scalar; neither may leak a numeric slot as a control character.
            return (CellText::Empty, 0);
        }
        let text = if raw.ch == 0 {
            CellText::Empty
        } else if raw.ch == Cell::CLUSTER {
            extra
                .and_then(|item| item.cluster.as_deref())
                .map_or(CellText::Empty, CellText::Cluster)
        } else {
            char::from_u32(raw.ch).map_or(CellText::Empty, CellText::Scalar)
        };
        (
            text,
            if raw.flags.contains(CellFlags::WIDE) {
                2
            } else {
                1
            },
        )
    }
}
