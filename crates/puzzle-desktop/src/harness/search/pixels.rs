//! Independent GPU checks for four authored hits, including a soft-wrap crossing.
//!
//! Each matched cell requires both exact paper and glyph ink. Every outside
//! pixel, including padding, must remain byte-identical to the ordinary frame.
//! These expectations come from fixture text, not SearchFill or render rects.
use super::SearchScene;
use puzzle_render::RgbaImage;
/// Names the four literal bar occurrences from the scenario's authored bytes.
fn hit(row: u32, col: u32) -> Option<usize> {
    // The third occurrence straddles the physical right margin.
    // Logical continuity must paint its final two cells on the following row.
    match (row, col) {
        (0, 4..=6) => Some(0),
        (0, 8..=10) => Some(1),
        (1, 11) | (2, 0..=1) => Some(2),
        (3, 4..=6) => Some(3),
        _ => None,
    }
}
/// Rejects blank text, wrong paper, escaped fills and stale clear/current state.
pub(super) fn check(scene: &SearchScene, image: &RgbaImage) -> Result<(), String> {
    let base = scene.baseline.as_ref().ok_or("search baseline missing")?;
    // Empty search restores the original image in every cell, including old hits.
    let state = scene.session.search_state();
    // Cell dimensions follow DPR; padding is the fixed authored 16-device-px inset.
    let metrics = scene.context.metrics();
    let (w, h) = (metrics.width_px, metrics.height_px);
    // Separate counters reject a flat rectangle that accidentally erases glyphs.
    let mut paper = [[0; 12]; 4];
    let mut ink = [[0; 12]; 4];
    // Inspect every RGBA pixel, rather than sparse paper samples near a glyph.
    for y in 0..image.height {
        for x in 0..image.width {
            // Readback rows are tightly packed, with four bytes for each pixel.
            let at = ((y * image.width + x) * 4) as usize;
            // Only the grid belongs to hit testing; padding remains unchanged.
            let cell = (x >= 16 && y >= 16 && x < 16 + 12 * w && y < 16 + 4 * h)
                .then(|| ((y - 16) / h, (x - 16) / w));
            // Match expectations are independent of engine output and frame rects.
            let matched = cell
                .and_then(|(row, col)| hit(row, col))
                .filter(|_| state.count > 0);
            if let (Some(index), Some((row, col))) = (matched, cell) {
                // Current index refers to query order, including two same-row hits.
                let expected = if state.current == Some(index) {
                    [119, 85, 51]
                } else {
                    [51, 68, 85]
                };
                // Exact token-colored pixels prove paper; all others require ink.
                if image.pixels[at..at + 3] == expected {
                    paper[row as usize][col as usize] += 1;
                } else {
                    ink[row as usize][col as usize] += 1;
                }
            // The baseline includes glyph antialiasing as well as default paper.
            // A shifted glyph or escaped fill therefore fails outside its cells.
            } else if image.pixels[at..at + 4] != base.pixels[at..at + 4] {
                return Err(format!("search changed outside pixel {x},{y}"));
            }
        }
    }
    // All twelve written match cells must retain ink and their own token paper.
    for row in 0..4 {
        for col in 0..12 {
            if state.count > 0
                && hit(row, col).is_some()
                && (paper[row as usize][col as usize] == 0 || ink[row as usize][col as usize] == 0)
            {
                // Count > 0 guarantees all four authored hits should be painted.
                return Err(format!("search paper or glyph ink missing {row},{col}"));
            }
        }
    }
    Ok(())
}
