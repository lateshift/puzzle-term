//! One D-27 row from a real hinted or unhinted outline.
//!
//! Each size makes a fresh atlas so the key for a character cannot return
//! a shape prepared for a different ppem. The same packer and CPU oracle
//! sample every resulting handle. Only the F variants change the final
//! coverage-to-color conversion, and only E sets the oracle's W flag.
//!
//! Text and labels share a baseline at an integer device coordinate.
//! This is the terminal placement convention from §8.2. The cap-height
//! variant changes ppem by at most three percent, then skrifa hints at
//! that exact ppem; no post-hoc stretch of a hinted bitmap is introduced.
use super::Context;
use crate::slug::hinting::{
    render,
    variant::{Blend, Variant, snapped_size},
};
use puzzle_slug::{SlugAtlas, reference::OracleImage};
use puzzle_text::probe::ProbeFont;

/// Draws all requested sizes for one variant, counting specimen ink only.
pub(super) fn draw_variant(
    context: &Context<'_>,
    index: usize,
    variant: Variant,
    image: &mut OracleImage,
) -> Result<usize, String> {
    // Sum only specimen coverage, not labels, so a broken outline pipeline
    // cannot pass the mechanical check through its row annotation.
    let mut ink = 0;
    for (size_index, size) in context.sizes.iter().copied().enumerate() {
        // Every variant owns a contiguous block of sizes. This ordering
        // keeps the 3× evidence crop a plain rectangle in the full sheet.
        let row = index * context.sizes.len() + size_index;
        // Baselines are integral device pixels for every branch. A and B/C
        // therefore differ in outline only, while D adds the cap-size nudge.
        let baseline = row as u32 * render::ROW_HEIGHT + 23;
        // Cap-height snapping changes the ppem supplied to the hinter;
        // scaling the hinted result afterward would erase its pixel intent.
        let snap = if variant.snap_cap {
            snapped_size(size, context.cap)
        } else {
            // Unsnapped rows are the control group: their ppem is exactly
            // the requested size, including B/C interpreter grid fitting.
            crate::slug::hinting::variant::Snap {
                size: size as f32,
                refused_percent: None,
            }
        };
        let size_px = snap.size;
        // Hinted outlines depend on ppem, so each row gets a fresh atlas;
        // reusing a character key from another size would compare wrong data.
        let mut atlas = SlugAtlas::default();
        let glyphs = match variant.hint {
            Some(mode) => {
                // Skrifa's own instructions or auto-hinter change only the
                // outline fed into the common Slug preparation pipeline.
                let hinted = context
                    .font
                    .hinted(size_px, mode)
                    .map_err(|error| error.to_string())?;
                render::prepare(&mut atlas, context.text, |ch| {
                    hinted.glyph(ch).map_err(|error| error.to_string())
                })?
            }
            // A preserves the size-independent, unhinted em-space path
            // from the M2 specimen for the baseline comparison.
            None => render::prepare(&mut atlas, context.text, |ch| {
                context.font.glyph(ch).map_err(|error| error.to_string())
            })?,
        };
        // The origin remains the same across branches; each glyph advance
        // is rounded by the shared draw_text path, not by either hinter.
        let position = render::Position {
            left: render::TEXT_LEFT,
            baseline,
        };
        let style = render::Style {
            size: size_px,
            blend: variant.blend,
            weighted: variant.weight,
            panel_width: context.panel_width,
        };
        // Verify every size row separately: aggregate ink could hide a
        // single blank row among four otherwise successful sizes.
        let row_ink = render::draw_text(&atlas, &glyphs, context.text, position, style, image)?;
        if row_ink == 0 {
            return Err(format!(
                "D-27 variant {} at {size}px has no specimen ink",
                variant.name
            ));
        }
        ink += row_ink;
        // The label is drawn afterward, and its coverage is excluded from
        // the specimen ink count even though it uses the same CPU oracle.
        draw_label(
            context.font,
            variant.name,
            size,
            snap.refused_percent,
            baseline,
            context.panel_width,
            image,
        )?;
    }
    Ok(ink)
}

/// Labels use the same CPU oracle at a fixed 14 px, regardless of variant.
fn draw_label(
    font: &ProbeFont<'_>,
    name: &str,
    size: u32,
    refused_percent: Option<f32>,
    baseline: u32,
    panel_width: u32,
    image: &mut OracleImage,
) -> Result<(), String> {
    // Labels stay at 14 px regardless of the specimen size, so reviewers
    // can read them without interpreting a second size-dependent treatment.
    let label = match refused_percent {
        Some(percent) => format!("{name} {size} no snap (needs {percent:.2}%)"),
        None => format!("{name} {size}"),
    };
    // They use unhinted glyphs and gamma blending only; applying the row's
    // experimental style to its label would bias visual comparison.
    let mut atlas = SlugAtlas::default();
    let glyphs = render::prepare(&mut atlas, &label, |ch| {
        font.glyph(ch).map_err(|error| error.to_string())
    })?;
    let position = render::Position { left: 12, baseline };
    let style = render::Style {
        size: 14.0,
        blend: Blend::Gamma,
        weighted: false,
        panel_width,
    };
    // Both panels receive the same label at corresponding pixel origins.
    render::draw_text(&atlas, &glyphs, &label, position, style, image)?;
    Ok(())
}
