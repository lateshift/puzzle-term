//! Logical-pixel queries over backend-neutral placements (design §11.1, D-28).
//!
//! Grid positions and glyph ink have separate records: transforms may move
//! ink, but never a cell. FrameBuilder fills this reused object from its
//! cached placements, including rows that needed no shaping this refresh.
//!
//! Logical coordinates remain stable as DPR changes. A cell rectangle uses
//! snapped device-cell metrics divided by DPR, while ink bounds come from
//! the selected backend. Geometry retains the source column separately from
//! a combining mark's offset, so moving ink never changes cell hit testing.
//!
//! Visible IDs are stored in painter/document order rather than assuming
//! a dense numeric range. Row insertions and deletions can leave ID gaps.
//! Inclusive ranges produce one rectangle per row segment, preserving the
//! caller's scratch allocation; no helper allocates a list behind its back.
//! Exclusive rectangle edges give adjacent cells an unambiguous hit owner.
use puzzle_vt::tracked::LineId;

/// Axis-aligned logical-pixel rectangle, with exclusive right/bottom edges.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// Left and top edges relative to the terminal surface.
    pub origin: [f32; 2],
    /// Width and height, independent of physical display scale.
    pub size: [f32; 2],
}
/// Logical rectangle operations at the public geometry boundary.
impl Rect {
    /// Positive-area intersection; touching edges and empty rectangles do not overlap.
    pub fn overlaps(self, other: Self) -> bool {
        // Corner containment is insufficient: a thin strip can cross a panel
        // without containing any corner. Intersect both axis intervals instead.
        (0..2).all(|axis| {
            self.size[axis] > 0.0
                && other.size[axis] > 0.0
                && self.origin[axis] < other.origin[axis] + other.size[axis]
                && other.origin[axis] < self.origin[axis] + self.size[axis]
        })
    }

    /// Tests an exclusive boundary so adjoining regions cannot both claim a point.
    pub fn contains(self, point: [f32; 2]) -> bool {
        (0..2).all(|axis| {
            point[axis] >= self.origin[axis] && point[axis] < self.origin[axis] + self.size[axis]
        })
    }
    /// Converts the whole rectangle to device pixels at the final render seam.
    pub fn device(self, dpr: f32) -> Self {
        Self {
            origin: self.origin.map(|v| v * dpr),
            size: self.size.map(|v| v * dpr),
        }
    }
}
/// A stable cell address, shared by geometry, decorations and hit tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cell {
    /// Retained VT row identity, rather than a transient screen row number.
    pub line: LineId,
    /// Zero-based column within that row.
    pub column: usize,
}
/// Inclusive cells at both ends, matching terminal selection intervals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Range {
    /// First addressed cell.
    pub start: Cell,
    /// Last addressed cell.
    pub end: Cell,
}
/// Placement metadata retained before backend selection changes the ink.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    /// First source cell of the shaping cluster.
    pub cell: Cell,
    /// Number of source cells occupied by the complete cluster.
    pub cells: usize,
    /// Typeface identity, including reserved platform cluster sources.
    pub face: u32,
    /// Typeface glyph or complete platform-cluster identity.
    pub glyph_id: u32,
    /// Baseline origin from layout, in logical surface pixels.
    pub baseline: [f32; 2],
    /// Backend-reported ink bounds, independent of cell advances.
    pub ink: Rect,
}
/// One currently visible row and its logical top edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    /// Source version of the text acknowledged by presentation.
    pub version: u64,
    /// Stable document identity.
    pub id: LineId,
    /// Logical y position, including the current fractional scroll displacement.
    pub y: f32,
}
/// Result of a geometry-only hit, before interactive regions are consulted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// Cell under the pointer; padding and out-of-grid points return None.
    pub cell: Option<Cell>,
    /// Index of the topmost glyph whose ink contains the point.
    pub glyph: Option<usize>,
    /// Rectangle containing this physical cell, when OSC 66 reserves it.
    pub block: Option<BlockExtent>,
}
/// Visible physical footprint of one indivisible text block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockExtent {
    /// Inclusive physical cells; the first visible fragment acts as owner.
    pub cells: Range,
    /// Visible logical-pixel extent, clipped at the grid edges.
    pub rect: Rect,
}
/// Reused per-frame geometry; callers borrow it only for this presentation.
#[derive(Default)]
pub struct TextGeometry {
    /// Viewport rows, in actual document order (IDs may have gaps).
    pub lines: Vec<Line>,
    /// One entry per neutral placement, never per COLR paint layer.
    pub placements: Vec<Glyph>,
    /// Sparse rectangular text; empty storage costs no allocation before first use.
    pub blocks: Vec<BlockExtent>,
    /// Logical width and height of one cell.
    pub cell: [f32; 2],
    /// Logical outer/top padding; origin() also includes any reserved left UI cells.
    pub inset: f32,
    /// Active grid width in columns.
    pub columns: usize,
    /// Physical display scale, used only at the renderer boundary.
    pub dpr: f32,
    /// Exact logical grid scissor copied from the renderer's device rectangle.
    /// Fractional scroll may add a row but never enlarges this clipping area.
    pub grid: Rect,
    /// Logical surface extent, including padding.
    pub viewport: Rect,
}
/// Geometry delivery preserves the receiving owner's warmed vector allocations.
impl Clone for TextGeometry {
    /// First ownership transfer allocates independently of the renderer's live buffers.
    fn clone(&self) -> Self {
        let mut geometry = Self::default();
        geometry.clone_from(self);
        geometry
    }
    /// Presented-frame updates reuse rows and placement capacity (P1).
    fn clone_from(&mut self, source: &Self) {
        // Derived clone_from would replace the entire struct and allocate both vectors.
        self.lines.clone_from(&source.lines);
        self.placements.clone_from(&source.placements);
        self.blocks.clone_from(&source.blocks);
        // Remaining geometry consists entirely of bounded copy-only values.
        self.cell = source.cell;
        self.inset = source.inset;
        self.columns = source.columns;
        self.dpr = source.dpr;
        self.grid = source.grid;
        self.viewport = source.viewport;
    }
}
/// Queries borrow reused placement storage and preserve caller scratch buffers.
impl TextGeometry {
    /// Logical text origin includes any left gutter, independent of top padding.
    pub fn origin(&self) -> [f32; 2] {
        [self.grid.origin[0].max(self.inset), self.inset]
    }
    /// Returns the current logical cell dimensions.
    pub fn cell_size(&self) -> [f32; 2] {
        self.cell
    }
    /// Iterates current row identities without allocating an ID list.
    pub fn visible_lines(&self) -> impl Iterator<Item = LineId> + '_ {
        self.lines.iter().map(|line| line.id)
    }
    /// Resolves one retained row to its current logical grid rectangle.
    pub fn line_rect(&self, id: LineId) -> Option<Rect> {
        let row = self.lines.iter().find(|line| line.id == id)?;
        Some(Rect {
            origin: [self.origin()[0], row.y],
            size: [self.columns as f32 * self.cell[0], self.cell[1]],
        })
    }
    /// Returns a cell rectangle without consulting a glyph's ink bounds.
    pub fn cell_rect(&self, pos: Cell) -> Option<Rect> {
        if pos.column >= self.columns {
            return None;
        }
        let mut rect = self.line_rect(pos.line)?;
        rect.origin[0] += pos.column as f32 * self.cell[0];
        rect.size[0] = self.cell[0];
        Some(rect)
    }
    /// Returns a whole visible block at a physical cell, otherwise that cell.
    pub fn text_rect(&self, pos: Cell) -> Option<Rect> {
        let cell = self.cell_rect(pos)?;
        // IL/DL reorder identities: physical geometry determines rectangle membership.
        let center = [
            cell.origin[0] + cell.size[0] * 0.5,
            cell.origin[1] + cell.size[1] * 0.5,
        ];
        self.blocks
            .iter()
            .find(|block| block.rect.contains(center))
            .map(|block| block.rect)
            .or(Some(cell))
    }
    /// Borrows all neutral glyph placements belonging to a visible row.
    pub fn glyphs(&self, id: LineId) -> impl Iterator<Item = &Glyph> {
        self.placements
            .iter()
            .filter(move |glyph| glyph.cell.line == id)
    }
    /// Returns reserved left UI cells, or legacy outer padding when none are reserved.
    pub fn gutter_rect(&self, id: LineId) -> Option<Rect> {
        let mut rect = self.line_rect(id)?;
        // Explicit UI cells exclude outer padding; legacy decoration gutters keep it.
        let width = self.origin()[0] - self.inset;
        rect.origin[0] = if width > 0.0 { self.inset } else { 0.0 };
        rect.size[0] = if width > 0.0 { width } else { self.inset };
        Some(rect)
    }
    /// Appends one rectangle per intersecting row, retaining caller capacity.
    pub fn range_rects(&self, range: Range, out: &mut Vec<Rect>) {
        // Actual document order matters after IL/DL; compare visible indices,
        // using the available endpoint to clip ranges entering from history.
        let first = self
            .lines
            .iter()
            .position(|line| line.id == range.start.line);
        let last = self.lines.iter().position(|line| line.id == range.end.line);
        for (index, line) in self.lines.iter().enumerate() {
            if first.is_some_and(|start| index < start)
                || last.is_some_and(|end| index > end)
                || (first.is_none()
                    && last.is_none()
                    && !(range.start.line <= line.id && line.id <= range.end.line))
            {
                continue;
            }
            let start = if Some(index) == first {
                range.start.column
            } else {
                0
            };
            let end = if Some(index) == last {
                range.end.column.saturating_add(1)
            } else {
                self.columns
            };
            if end > start
                && let Some(mut rect) = self.line_rect(line.id)
            {
                rect.origin[0] += start as f32 * self.cell[0];
                rect.size[0] =
                    (end.min(self.columns) - start.min(self.columns)) as f32 * self.cell[0];
                out.push(rect);
            }
        }
    }
    /// Resolves cells from the grid and ink from the placement records.
    pub fn hit_test(&self, point: [f32; 2]) -> Hit {
        let cell = self.lines.iter().find_map(|line| {
            let rect = self.line_rect(line.id)?;
            rect.contains(point).then(|| Cell {
                line: line.id,
                column: ((point[0] - self.origin()[0]) / self.cell[0]) as usize,
            })
        });
        let glyph = self
            .placements
            .iter()
            .rposition(|glyph| glyph.ink.contains(point));
        let block = self
            .blocks
            .iter()
            .find(|block| block.rect.contains(point))
            .copied();
        Hit { cell, glyph, block }
    }
}
