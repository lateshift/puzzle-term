//! Borrowed fallback outlines obey D-27 through the neutral engine.
//! The primary face supplies cell dimensions; a fallback's metrics are used
//! only to normalize its own outline. They never resize the terminal grid.
//! An attachment translation is applied after normalization, preserving the
//! shaper's baseline-relative mark coordinates under both extraction modes.
//! Outline storage can therefore be released independently of discovery state.
//!
//! An autohinter exists only during cold extraction. It borrows no native types
//! and emits device coordinates normalized back to Slug ems. Transformed ink
//! arrives with an unhinted key, so pixel fitting never survives an effect.
//! Shaper attachment offsets are em coordinates; font advances remain ignored.
//! A missing outline fails the candidate rather than publishing partial ink.
use crate::glyph::{GlyphPlacement, HintMode};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{Location, Size},
    outline::{DrawSettings, Engine, HintingInstance, HintingOptions, SmoothMode, Target},
};

/// Extracts every visible component into one baseline-relative owned path.
pub(super) fn outlines(
    font: &FontRef<'_>,
    location: &Location,
    glyphs: &[(u32, f32, f32)],
    p: &GlyphPlacement,
) -> Result<kurbo::BezPath, String> {
    // The same units-per-em scales marks and curves, even in TTC subfaces.
    let upem = font.metrics(Size::unscaled(), location).units_per_em;
    if upem == 0 {
        return Err("system outline has zero units per em".into());
    }
    // Only the outline collection is opened; unrelated large font tables stay lazy.
    let collection = font.outline_glyphs();
    // One hinter serves all components of this cold cluster, then is released.
    let hint = hinter(&collection, location, p)?;
    // Hinted draw settings emit pixels, while unhinted settings emit font units.
    let scale = if hint.is_some() {
        1.0 / f64::from(p.key.hint_ppem)
    } else {
        1.0 / f64::from(upem)
    };
    // A complete cluster is one owned shape; failed extraction drops every component.
    let mut combined = kurbo::BezPath::new();
    // Combining marks carry attachment offsets rather than independent cell advances.
    for &(gid, x, y) in glyphs {
        // Missing outlines cannot be replaced with .notdef during fallback validation.
        let glyph = collection
            .get(skrifa::GlyphId::new(gid))
            .ok_or("system glyph has no outline")?;
        // The pen normalizes while drawing, avoiding a second walk over the outline.
        let mut pen = crate::probe::EmPen {
            path: kurbo::BezPath::new(),
            scale,
        };
        // Explicit hinted settings forbid the engine from selecting another strategy.
        let settings = match &hint {
            Some(instance) => DrawSettings::hinted(instance, false),
            None => DrawSettings::unhinted(Size::unscaled(), location),
        };
        // Font bytes and the hinter remain local until all contours are owned.
        glyph.draw(settings, &mut pen).map_err(|e| e.to_string())?;
        // Offset signs stay y-up until the Slug instance applies the device y-flip.
        let shifted = kurbo::Affine::translate((f64::from(x), f64::from(y))) * pen.path;
        // Each contour retains its original order and winding under attachment translation.
        combined.extend(shifted.iter());
    }
    // Empty substitutions provide no visible coverage even with valid cmap entries.
    if combined.elements().is_empty() {
        return Err("system cluster has no visible outline".into());
    }
    // This path no longer borrows the font bytes and can survive unmapped candidates.
    Ok(combined)
}
/// Hint cache suffixes specify an explicit engine; there is no silent native fallback.
fn hinter(
    glyphs: &skrifa::outline::OutlineGlyphCollection<'_>,
    location: &Location,
    p: &GlyphPlacement,
) -> Result<Option<HintingInstance>, String> {
    // Zero suffix is the size-independent/effect path selected above this seam.
    if p.key.hint_ppem == 0 || p.key.hint_mode == HintMode::None {
        return Ok(None);
    }
    // Light and native have distinct cache keys and must use distinct engines.
    let options = HintingOptions {
        engine: if p.key.hint_mode == HintMode::Light {
            Engine::Auto(None)
        } else {
            Engine::Interpreter
        },
        // Smooth light moves vertical alignment without forcing LCD color coverage.
        target: Target::from(if p.key.hint_mode == HintMode::Light {
            SmoothMode::Light
        } else {
            SmoothMode::Normal
        }),
    };
    // One cold cluster can contain several mark outlines using the same hinter.
    HintingInstance::new(
        glyphs,
        Size::new(f32::from(p.key.hint_ppem)),
        location,
        options,
    )
    // A setup error rejects this font candidate; it never escapes as a panic.
    .map(Some)
    .map_err(|e| e.to_string())
}
