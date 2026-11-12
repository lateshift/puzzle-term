//! Procedural terminal glyphs drawn from integer device-cell geometry.
//!
//! Font outlines cannot promise that two neighbouring box cells touch
//! after scaling. These glyphs instead extend their filled rectangles to
//! the exact left or right cell boundary. Rectangles and dots begin in
//! device pixels, then enter Slug in em coordinates using cell height as
//! the common scale. The shared edge therefore has continuous ink even
//! when a font's glyph advance differs from the grid cell width.
//!
//! The construction is deliberately a filled outline, never a device-line
//! stroke with a floating centre. Filled pixel rectangles express the
//! exact half-open coverage of a cell edge. The Slug atlas then stores
//! these contours like font glyphs, allowing one GPU path for both kinds.
use kurbo::{BezPath, Point};

/// Reserved face ID for a procedural glyph prepared through the normal seam.
pub const BUILTIN_FACE: u32 = u32::MAX;
/// Reserved face ID for a codepoint missing from the entire fallback chain.
pub const TOFU_FACE: u32 = u32::MAX - 1;
/// Shared border/digit identities use admission without pretending to be font glyphs.
pub const TOFU_COMPONENT_FACE: u32 = u32::MAX - 5;

/// Half blocks, eighths, quadrants and the three shade levels.
mod blocks;
/// Heavy, light and double box strokes assembled from edge arms.
mod box_lines;
/// Unicode Braille cells and the legacy six-dot block forms.
mod dots;
/// Powerline separators and the last-resort hex-code box.
mod symbols;
/// Shared border and digit paths bound tofu atlas usage independently of the corpus.
mod tofu;

/// Device-pixel measurements used to generate a cell-specific outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BuiltinCell {
    /// Exact number of device pixels across one terminal column.
    pub width: u16,
    /// Exact number of device pixels down one terminal row.
    pub height: u16,
    /// Light-line thickness, already snapped to device pixels.
    pub stroke: u16,
}

/// Vector ink and optional shade opacity for a builtin character.
pub struct BuiltinInk {
    /// Outline in em units with em = cell height in device pixels.
    pub path: BezPath,
    /// Multiplicative opacity; 25/50/75 % shades use solid coverage.
    pub alpha: u8,
}

/// Adds pixel-aligned contours while converting y-down cells to y-up ems.
struct PixelPen {
    /// Combined Slug path for one procedural character.
    path: BezPath,
    /// Cell height is the em-to-pixel scale during rendering.
    height: f64,
}

/// Makes bounded cell geometry before any Slug shape is prepared.
impl BuiltinCell {
    /// Rejects dimensions that cannot describe a visible terminal cell.
    pub fn new(width: u32, height: u32, stroke: u32) -> Result<Self, String> {
        // External cell settings may exceed the packed 16-bit identity.
        // Rejecting them here avoids colliding cache entries downstream.
        if width == 0 || height == 0 || stroke == 0 {
            return Err("builtin cell dimensions must be positive".into());
        }
        Ok(Self {
            width: u16::try_from(width).map_err(|_| "builtin cell is too wide")?,
            height: u16::try_from(height).map_err(|_| "builtin cell is too tall")?,
            stroke: u16::try_from(stroke).map_err(|_| "builtin stroke is too wide")?,
        })
    }

    /// Packs dimensions into a glyph instance key without font byte state.
    pub fn pack(self) -> Result<u32, String> {
        if self.width > 8191 || self.height > 8191 || self.stroke > 63 {
            return Err("builtin cell exceeds packed geometry limits".into());
        }
        // The same cell dimensions must produce one reusable Slug handle.
        Ok(u32::from(self.width) | (u32::from(self.height) << 13) | (u32::from(self.stroke) << 26))
    }

    /// Decodes a previously validated key before procedural shape lookup.
    pub fn unpack(value: u32) -> Result<Self, String> {
        Self::new(value & 0x1fff, (value >> 13) & 0x1fff, value >> 26)
    }

    /// Reports whether a scalar belongs to an §8.6 procedural range.
    pub fn supports(ch: char) -> bool {
        matches!(ch as u32, 0x2500..=0x259f | 0x2800..=0x28ff | 0x1fb00..=0x1fb3b | 0xe0a0..=0xe0a3 | 0xe0b0..=0xe0bf)
    }

    /// Produces only the characters assigned to procedural geometry in §8.6.
    pub fn ink(self, ch: char) -> Option<BuiltinInk> {
        // One pen accumulates disjoint contours for compound symbols.
        // The alpha is separate because shade glyphs have solid geometry.
        let mut pen = PixelPen {
            path: BezPath::new(),
            height: f64::from(self.height),
        };
        let code = ch as u32;
        // Dispatch by the Unicode ranges named in §8.6. Ordinary text
        // returns None so it keeps using the selected OpenType face.
        let alpha = match code {
            0x2500..=0x257f => box_lines::draw(&mut pen, self, code),
            0x2580..=0x259f => blocks::draw(&mut pen, self, code),
            0x2800..=0x28ff => dots::braille(&mut pen, self, code),
            0x1fb00..=0x1fb3b => dots::sextant(&mut pen, self, code),
            0xe0a0..=0xe0a3 | 0xe0b0..=0xe0bf => symbols::powerline(&mut pen, self, code),
            _ => return None,
        };
        Some(BuiltinInk {
            path: pen.path,
            alpha,
        })
    }

    /// Draws a missing-codepoint box without invoking a broken font.
    pub fn tofu(self, code: u32) -> BuiltinInk {
        // Tofu does not depend on font loading or fallback, so this path
        // remains available even when every supplied font was rejected.
        let mut pen = PixelPen {
            path: BezPath::new(),
            height: f64::from(self.height),
        };
        symbols::tofu(&mut pen, self, code);
        BuiltinInk {
            path: pen.path,
            alpha: 255,
        }
    }
}

/// Converts filled pixel shapes to a device-size-independent Slug path.
impl PixelPen {
    /// Appends one half-open integer pixel rectangle, clipped to the cell.
    fn rect(&mut self, cell: BuiltinCell, x0: i32, y0: i32, x1: i32, y1: i32) {
        // Clipping ensures a badly sized stroke cannot draw outside the
        // source cell and obscure the following terminal column.
        let width = i32::from(cell.width);
        let height = i32::from(cell.height);
        let (x0, x1) = (x0.clamp(0, width), x1.clamp(0, width));
        let (y0, y1) = (y0.clamp(0, height), y1.clamp(0, height));
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        // Each rectangle reaches the exact cell edge when its source does.
        // Slug's anti-aliasing does not invent a gap between coincident edges.
        let a = self.point(x0 as f64, y0 as f64);
        let b = self.point(x1 as f64, y1 as f64);
        self.path.move_to((a.x, a.y));
        self.path.line_to((b.x, a.y));
        self.path.line_to((b.x, b.y));
        self.path.line_to((a.x, b.y));
        self.path.close_path();
    }

    /// Appends a four-corner diagonal or Powerline polygon in cell pixels.
    fn polygon(&mut self, points: &[(f64, f64)]) {
        // Powerline polygons may touch a boundary but have no stroke
        // width; their filled vertices control the neighbour join.
        let Some(&(x, y)) = points.first() else {
            return;
        };
        let first = self.point(x, y);
        self.path.move_to(first);
        for &(x, y) in &points[1..] {
            self.path.line_to(self.point(x, y));
        }
        self.path.close_path();
    }

    /// Maps x-right/y-down device pixels into Slug's x-right/y-up ems.
    fn point(&self, x: f64, y: f64) -> Point {
        Point::new(x / self.height, 1.0 - y / self.height)
    }
}

#[cfg(test)]
mod tests;
