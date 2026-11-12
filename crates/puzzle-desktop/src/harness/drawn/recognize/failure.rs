//! Exact failed-cell observations survive retries without rerendering templates.
//!
//! A second GPU submission cannot diagnose an intermittent first submission:
//! it may render different pixels. The classifier therefore retains the masks
//! it actually scored, alongside the attachment crop. Only uncertain or wrong
//! cells pay for this diagnostic copy; the ordinary passing sheet is discarded.
//! Files are written into the harness's never-reused failure directory.
use super::*;
use std::{fs, path::Path};

/// Serializes the precise color projection and candidate set used for one cell.
///
/// `pos` and `size` are physical row/column and device-pixel cell dimensions.
/// RGB is retained as well as coverage so a bad normalization axis is visible.
/// The template sheet is borrowed before its recognition pass releases it.
/// No threshold or candidate is altered by this observation.
pub(super) fn cell(
    sheet: &Sheet,
    pixels: &[[u8; 3]],
    observed: &[u8],
    pos: (usize, usize),
    size: (u32, u32),
) -> serde_json::Value {
    // Keep all competitors, including the correct glyph if another label wins.
    // An error averaged over device area is the classifier's existing unit.
    let scores = sheet
        .chars
        .iter()
        .zip(&sheet.masks)
        .map(|(ch, mask)| {
            let error = observed
                .iter()
                .zip(mask)
                .map(|(a, b)| u32::from(a.abs_diff(*b)))
                .sum::<u32>();
            serde_json::json!({
                "character": ch.to_string(),
                "error": error as f64 / observed.len().max(1) as f64,
                "coverage": mask
            })
        })
        .collect::<Vec<_>>();
    // Coordinates locate the crop in warm.png; sentinels expose any foreground
    // or background mismatch independently of the stored terminal character.
    serde_json::json!({
        "row": pos.0, "column": pos.1, "width": size.0, "height": size.1,
        "observed_rgb": pixels, "observed_coverage": observed,
        "blank": sheet.blank, "block": sheet.block, "candidates": scores
    })
}

/// Writes frozen diagnostic values; this path performs no further GPU work.
pub(in crate::harness) fn capture(state: &Scenario, path: &Path) -> Result<(), String> {
    let Some(readback) = &state.drawn_cache else {
        // Nonrecognition failures still retain their PNG/grid/transcript bundle.
        return Ok(());
    };
    for evidence in &readback.failures {
        // The row and column came from bounded grid traversal in this process.
        // Separate files keep one uncertain cell inspectable without reading a
        // large full-screen JSON dump or discarding other failed cells.
        let row = evidence["row"].as_u64().ok_or("missing failure row")?;
        let column = evidence["column"]
            .as_u64()
            .ok_or("missing failure column")?;
        fs::write(
            path.join(format!("cell-{row}-{column}.json")),
            evidence.to_string(),
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}
