//! Unhinted font outlines for the M2 CPU-oracle specimen (design §8.5).
//!
//! Font coordinates arrive in font units with y pointing up. The pen divides
//! by units per em before it hands a kurbo path to `puzzle-slug`; one packed
//! outline can then be sampled at every specimen size and DPR.
//!
//! The probe is deliberately narrow. M2 needs real outlines to evaluate the
//! reference rasterizer, but M4 owns the full font system: loading policy,
//! variation selection, shaping, metrics and fallback. Keeping these here
//! respects the leaf `puzzle-slug` boundary in design §4.1.
//!
//! `Size::unscaled()` asks skrifa for design-unit coordinates. Slug texture
//! data is em-space, so the scale belongs in the pen before kurbo's cubic
//! conversion and bbox calculation. Scaling after conversion would change
//! the meaning of its fixed 1/2048-em tolerance (design §8.5).
//!
//! Font bytes are untrusted even in a diagnostic tool. Missing tables,
//! characters or outlines return a report. The caller owns the byte slice
//! for as long as the borrowed face reads from it.
use kurbo::BezPath;
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
    outline::{DrawSettings, OutlinePen},
};

/// Size-specific interpreter and auto-hinter shared with future atlas work.
mod hint;
pub use hint::{HintedProbe, ProbeHintMode};

#[cfg(test)]
mod tests;

/// A parse, character-map or outline failure in an external font file.
#[derive(Debug)]
pub enum ProbeError {
    /// The input bytes are not a usable OpenType face.
    Font,
    /// The face has no outline for the requested scalar.
    MissingGlyph(char),
    /// The outline interpreter rejected the glyph.
    Outline,
}

impl std::fmt::Display for ProbeError {
    /// Gives CLI callers a short cause without leaking parser internals.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Font => write!(f, "invalid font face or units per em"),
            Self::MissingGlyph(ch) => write!(f, "font has no glyph for {ch:?}"),
            Self::Outline => write!(f, "font outline could not be drawn"),
        }
    }
}

/// Lets the error cross the library/xtask boundary normally.
impl std::error::Error for ProbeError {}

/// One parsed face borrowed from validated font bytes.
pub struct ProbeFont<'a> {
    /// Skrifa reads tables lazily from these caller-owned bytes.
    font: FontRef<'a>,
    /// Font units become em coordinates before Slug preparation.
    em_scale: f64,
}

impl<'a> ProbeFont<'a> {
    /// Parses a face and rejects a zero units-per-em divisor.
    pub fn new(bytes: &'a [u8]) -> Result<Self, ProbeError> {
        let font = FontRef::new(bytes).map_err(|_| ProbeError::Font)?;
        let upem = font
            .metrics(Size::unscaled(), LocationRef::default())
            .units_per_em;
        if upem == 0 {
            return Err(ProbeError::Font);
        }
        Ok(Self {
            font,
            em_scale: 1.0 / f64::from(upem),
        })
    }

    /// Draws one character without hinting and returns its em advance.
    ///
    /// The specimen intentionally maps scalars without shaping. The full
    /// ligature and cluster layout belongs to M4, while the glyph outline
    /// and coverage quality can already be judged here.
    pub fn glyph(&self, ch: char) -> Result<(BezPath, f32), ProbeError> {
        // A missing character has no usable real-text specimen glyph; do
        // not silently substitute .notdef and mistake it for the target.
        let id = self
            .font
            .charmap()
            .map(ch)
            .ok_or(ProbeError::MissingGlyph(ch))?;
        let outline = self
            .font
            .outline_glyphs()
            .get(id)
            .ok_or(ProbeError::MissingGlyph(ch))?;
        let advance = self
            .font
            .glyph_metrics(Size::unscaled(), LocationRef::default())
            .advance_width(id)
            .ok_or(ProbeError::MissingGlyph(ch))?;
        // The path pen receives the actual quadratic or cubic commands;
        // no rasterization or hinting enters this diagnostic path.
        let mut pen = EmPen {
            path: BezPath::new(),
            scale: self.em_scale,
        };
        outline
            .draw(
                DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
                &mut pen,
            )
            .map_err(|_| ProbeError::Outline)?;
        // The width must use the same em scale as the outline so the sheet
        // can place glyphs on device-pixel origins at every requested size.
        Ok((pen.path, advance * self.em_scale as f32))
    }

    /// Gives a glyph's em advance even when a space has no outline.
    pub fn advance(&self, ch: char) -> Result<f32, ProbeError> {
        // The specimen must advance over spaces without inventing a visible
        // contour, so metrics are queried independently of outline lookup.
        let id = self
            .font
            .charmap()
            .map(ch)
            .ok_or(ProbeError::MissingGlyph(ch))?;
        let advance = self
            .font
            .glyph_metrics(Size::unscaled(), LocationRef::default())
            .advance_width(id)
            .ok_or(ProbeError::MissingGlyph(ch))?;
        Ok(advance * self.em_scale as f32)
    }

    /// Returns the face's cap height in ems for the §8.2 snap calculation.
    pub fn cap_height(&self) -> Option<f32> {
        let metrics = self.font.metrics(Size::unscaled(), LocationRef::default());
        metrics
            .cap_height
            .map(|height| height * self.em_scale as f32)
    }
}

/// Translates the outline callback into a reusable kurbo path in em space.
pub(crate) struct EmPen {
    /// Each contour stays explicit so the Slug converter can close it.
    pub(crate) path: BezPath,
    /// Both coordinates use the same factor, preserving curve geometry.
    pub(crate) scale: f64,
}

impl OutlinePen for EmPen {
    /// Begins an independent contour without connecting it to the last one.
    fn move_to(&mut self, x: f32, y: f32) {
        self.path
            .move_to((f64::from(x) * self.scale, f64::from(y) * self.scale));
    }

    /// Retains lines for Slug's `{p1,p2,p2}` encoding.
    fn line_to(&mut self, x: f32, y: f32) {
        self.path
            .line_to((f64::from(x) * self.scale, f64::from(y) * self.scale));
    }

    /// Retains TrueType's quadratic control point exactly.
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.path.quad_to(
            (f64::from(cx) * self.scale, f64::from(cy) * self.scale),
            (f64::from(x) * self.scale, f64::from(y) * self.scale),
        );
    }

    /// Kurbo later applies the required 1/2048-em cubic tolerance.
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.path.curve_to(
            (f64::from(cx0) * self.scale, f64::from(cy0) * self.scale),
            (f64::from(cx1) * self.scale, f64::from(cy1) * self.scale),
            (f64::from(x) * self.scale, f64::from(y) * self.scale),
        );
    }

    /// Marks the contour closed so winding survives packing.
    fn close(&mut self) {
        self.path.close_path();
    }
}
