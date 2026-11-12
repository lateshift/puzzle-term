//! Comparison sheet layout and evidence output for D-27.
//!
//! The full image aligns five device sizes below each variant label and
//! places the same oracle coverage on white and #1e1e1e. Row painting
//! lives in `rows` because it owns size-specific hinted atlases, while
//! this module bounds the image and writes its review artifacts.
//!
//! The width comes from unhinted design advances at the largest permitted
//! ppem, plus three percent for cap snapping. Hinted outlines may move ink
//! within that cell, but they cannot alter the layout advance. This keeps
//! all variants spatially comparable and follows the D-28 placement seam.
//!
//! A 3× crop is a nearest-neighbor expansion of one variant's five rows,
//! including both panels and labels. It is saved separately so an image
//! viewer's smooth zoom cannot erase a one-pixel artifact.
/// Renders the real-outline rows for one named variant.
mod rows;

use super::{render, variant::VARIANTS};
use puzzle_slug::reference::OracleImage;
use puzzle_text::probe::ProbeFont;
use std::{fs, path::Path};

/// Input and layout values shared by every comparison variant.
pub(super) struct Context<'a> {
    /// Validated font whose bytes outlive each hinted row.
    pub(super) font: &'a ProbeFont<'a>,
    /// Same diagnostic string across all rows.
    pub(super) text: &'a str,
    /// Device sizes, restricted to the requested 10–14 px range.
    pub(super) sizes: &'a [u32],
    /// Unscaled cap height used for the bounded snap.
    pub(super) cap: f32,
    /// Identical width of the light and dark panels.
    pub(super) panel_width: u32,
}

/// Creates the sheet and checks each variant for actual oracle ink.
pub(super) fn write(font: &ProbeFont<'_>, text: &str, sizes: &[u32]) -> Result<(), String> {
    let cap = font.cap_height().ok_or("font has no cap-height metric")?;
    if !cap.is_finite() || cap <= 0.0 {
        return Err("font cap-height metric is invalid".into());
    }
    let panel_width = panel_width(font, text)?;
    let context = Context {
        font,
        text,
        sizes,
        cap,
        panel_width,
    };
    let mut image = render::blank(panel_width, sizes.len() as u32 * VARIANTS.len() as u32)?;
    let mut ink_counts = Vec::with_capacity(VARIANTS.len());
    for (index, variant) in VARIANTS.iter().copied().enumerate() {
        // Counting the specimen excludes the label: a broken glyph path
        // cannot pass just because the row name was rendered successfully.
        let ink = rows::draw_variant(&context, index, variant, &mut image)?;
        if ink == 0 {
            return Err(format!(
                "D-27 variant {} contains no oracle ink",
                variant.name
            ));
        }
        ink_counts.push((variant.name, ink));
    }
    write_evidence(&image, sizes, &ink_counts)
}

/// Sizes both panels from the largest requested size and real font advances.
fn panel_width(font: &ProbeFont<'_>, text: &str) -> Result<u32, String> {
    let mut width = render::TEXT_LEFT + 24;
    for ch in text.chars() {
        let (_, advance) = font.glyph(ch).map_err(|error| error.to_string())?;
        width = width.saturating_add((advance * 14.42).ceil().max(1.0) as u32);
    }
    // The 3× crops multiply the two-panel width too; reject oversized
    // custom input before allocating tens of megabytes of pixels.
    if width.saturating_mul(6).saturating_add(72) > 4096 {
        return Err("comparison text is too wide for 3x evidence".into());
    }
    Ok(width)
}

/// Writes one full sheet and exact nearest-neighbor crops for each variant.
fn write_evidence(
    image: &OracleImage,
    sizes: &[u32],
    counts: &[(&str, usize)],
) -> Result<(), String> {
    let folder = Path::new("target/slug");
    fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    super::super::png::write(&folder.join("hinting-compare.png"), image)?;
    for (index, (name, ink)) in counts.iter().enumerate() {
        let first_row = index as u32 * sizes.len() as u32;
        let crop = render::crop_threefold(image, first_row, sizes.len() as u32);
        super::super::png::write(
            &folder.join(format!("hinting-{}-3x.png", name.to_lowercase())),
            &crop,
        )?;
        println!("variant={name} ink_pixels={ink}");
    }
    println!("oracle=target/slug/hinting-compare.png");
    Ok(())
}
