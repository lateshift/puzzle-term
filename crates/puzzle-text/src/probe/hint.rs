//! Skrifa hinting at a chosen device ppem for the D-27 probe (§8.9).
//!
//! Native hinting executes the face's own instructions; light auto-hinting
//! uses skrifa's distinct engine. Both yield device-pixel outline points.
//! We divide those points by ppem before passing them to the unchanged Slug
//! packer, so the normal em-space oracle can sample at that same ppem.
//!
//! This small, reusable adapter will also serve a future oracle-atlas
//! rasterizer if D-27 selects one. It does not choose which rendering
//! backend wins or alter the terminal's design-unit glyph advance.
//!
//! `HintingInstance` is configured once per row and reused for all its
//! characters. Skrifa may cache interpreter state inside it; rebuilding
//! the instance per glyph would compare setup overhead instead of outline
//! behavior. The row's byte slice remains borrowed by `ProbeFont`.
//!
//! The glyph's advance comes from unscaled metrics even for hinted curves.
//! This preserves grid placement while permitting the outline to shift
//! inside its cell, which is the visual question behind D-27 and D-28.
use super::{EmPen, ProbeError, ProbeFont};
use kurbo::BezPath;
use skrifa::{
    MetadataProvider,
    instance::{LocationRef, Size},
    outline::{DrawSettings, Engine, HintingInstance, HintingOptions, SmoothMode, Target},
};

/// The two real skrifa hinting strategies in the D-27 comparison.
#[derive(Clone, Copy, Debug)]
pub enum ProbeHintMode {
    /// Execute the font's own TrueType or PostScript instructions.
    Interpreter,
    /// Apply skrifa's auto-hinter with its light smooth target.
    AutoLight,
}

/// A ppem-specific hinter reused across the characters of one specimen row.
pub struct HintedProbe<'a> {
    /// The original face supplies charmap, metrics and outline lookup.
    font: &'a ProbeFont<'a>,
    /// Skrifa owns the size-specific interpreter or auto-hinter state.
    instance: HintingInstance,
    /// Hinted pixel coordinates return to em space before Slug packing.
    em_scale: f64,
}

/// Sets up one mode without allowing an automatic fallback between B and C.
impl<'a> ProbeFont<'a> {
    /// Configures a real skrifa hinter at the exact requested device ppem.
    /// A failed interpreter setup remains an error rather than silently
    /// substituting auto-hinting, because B and C must stay distinct.
    pub fn hinted(&'a self, ppem: f32, mode: ProbeHintMode) -> Result<HintedProbe<'a>, ProbeError> {
        if !ppem.is_finite() || ppem <= 0.0 {
            return Err(ProbeError::Font);
        }
        let options = match mode {
            // Never use AutoFallback here: its choice depends on font tables
            // and could make B and C unexpectedly identical.
            ProbeHintMode::Interpreter => HintingOptions {
                engine: Engine::Interpreter,
                target: Target::from(SmoothMode::Normal),
            },
            ProbeHintMode::AutoLight => HintingOptions {
                engine: Engine::Auto(None),
                target: Target::from(SmoothMode::Light),
            },
        };
        let instance = HintingInstance::new(
            &self.font.outline_glyphs(),
            Size::new(ppem),
            LocationRef::default(),
            options,
        )
        .map_err(|_| ProbeError::Outline)?;
        // DrawSettings emits positions in device pixels at this ppem; the
        // reciprocal restores Slug's em-space storage without re-hinting.
        Ok(HintedProbe {
            font: self,
            instance,
            em_scale: 1.0 / f64::from(ppem),
        })
    }
}

/// Extracts hinted outlines while preserving the grid's design advance.
impl HintedProbe<'_> {
    /// Draws one glyph in hinted device coordinates, then normalizes to ems.
    pub fn glyph(&self, ch: char) -> Result<(BezPath, f32), ProbeError> {
        // An unsupported scalar is reported rather than replaced with
        // .notdef, which could make a specimen row misleadingly nonblank.
        let id = self
            .font
            .font
            .charmap()
            .map(ch)
            .ok_or(ProbeError::MissingGlyph(ch))?;
        let outline = self
            .font
            .font
            .outline_glyphs()
            .get(id)
            .ok_or(ProbeError::MissingGlyph(ch))?;
        let advance = self
            .font
            .font
            .glyph_metrics(Size::unscaled(), LocationRef::default())
            .advance_width(id)
            .ok_or(ProbeError::MissingGlyph(ch))?;
        // Font metrics stay in design units even while the hinted outline
        // moves individual points onto the pixel grid.
        // The interpreter emits pixel-space points at this ppem. Dividing
        // by ppem lets the unchanged atlas and oracle work in em space.
        let mut pen = EmPen {
            path: BezPath::new(),
            scale: self.em_scale,
        };
        outline
            .draw(DrawSettings::hinted(&self.instance, false), &mut pen)
            .map_err(|_| ProbeError::Outline)?;
        Ok((pen.path, advance * self.font.em_scale as f32))
    }
}
