//! Per-cell ink witnesses from the actual live executor (M8 reviewer bisect).
//! Reuses the window's GPU atlas and geometry; no fresh builder, glyph preparation
//! or re-upload may repair missing data before this diagnostic observes it.
//!
//! The caller invokes this once after warm-up, never inside the timed scroll
//! interval. It records every authored letter separately because a partially
//! working atlas can produce a plausible nonblank screenshot. Both the PNG
//! and JSON stay under target/, outside packaged application resources.
//!
//! A viewport-local row and its source-text column offset identify each cell.
//! This accounts for the ordinary shell prompt on a one-row warm-up without
//! treating that prompt as neutral test evidence or copying its contents.
//! The source comes from the very slot used for the retained presentation. A faster
//! parser can already have scrolled later text into VT while this image stays visible.
//! Comparing current VT with retained pixels would mislabel that ordinary content
//! age as lost GPU ink. Snapshot cell access also preserves multi-scalar clusters
//! without reaching across to the Session worker or altering its parse order.
use super::LiveWindow;
use puzzle_term::{frame_snapshot::cell_from_row, vt::view::CellText};
use serde_json::json;
use std::path::Path;

/// Harness-only observation of the engine-owned surface.
pub(in crate::window) trait GlyphBisect {
    /// Runs the authored measurement using retained resources.
    fn glyph_bisect(&mut self) -> Result<(), String>;
    /// Retained neutral text for the authored alphabet witness.
    fn alphabet_rows(&self) -> Result<Vec<(u32, String, usize)>, String>;
}

impl GlyphBisect for LiveWindow {
    /// Saves the retained image and every authored alphabet cell's ink count.
    fn glyph_bisect(&mut self) -> Result<(), String> {
        // Temporarily omit even the last frame's dirty spans. A readback that
        // re-applies them could conceal a failed earlier surface submission.
        let uploads = std::mem::take(&mut self.frame.uploads);
        let read = self
            .frame_renderer
            .render_to_image(&self.renderer, &mut self.frame);
        self.frame.uploads = uploads;
        let (image, _) = read.map_err(|error| error.to_string())?;
        // Save exactly this retained atlas image before examining cell crops.
        // A new builder would silently regenerate the missing startup glyphs.
        crate::png::write(Path::new("target/m8-glyph-bisect.png"), &image)?;
        let metrics = self.text_context.metrics();
        // The first grid batch supplies device-pixel padding. Hard-coded
        // offsets would misdiagnose settings or DPR changes as missing letters.
        let clip = self
            .frame
            .batches
            .iter()
            .find_map(|batch| batch.scissor)
            .ok_or("no grid clip in retained frame")?;
        let rows = self.alphabet_rows()?;
        let pixels = &image;
        // Spaces intentionally contain no ink, so the witness covers every
        // alphabetic cell and preserves each count for the eight-way comparison.
        let cells: Vec<_> = rows
            .iter()
            .flat_map(|(row, text, offset)| {
                text.chars()
                    .enumerate()
                    .filter(|(_, ch)| ch.is_ascii_alphabetic())
                    .map(move |(column, ch)| {
                        let x = clip.x + (column + offset) as u32 * metrics.width_px;
                        let y = clip.y + row * metrics.height_px;
                        json!({"row":row,"column":column,"char":ch.to_string(),
                    "ink":ink(pixels,x,y,metrics.width_px,metrics.height_px)})
                    })
            })
            .collect();
        // Keep the full list as well as failures: a headline zero alone cannot
        // prove that all expected cells were actually inspected.
        let missing: Vec<_> = cells.iter().filter(|cell| cell["ink"] == 0).collect();
        let report = json!({"cells":cells,"missing":missing,"surface_device_px":self.frame.target_size,
            "metrics":[metrics.width_px,metrics.height_px],"clip":[clip.x,clip.y],
            "dpr":self.window.scale_factor(),"prepared_glyphs":self.text_context.prepared_glyphs()});
        std::fs::write("target/m8-glyph-bisect.json", report.to_string() + "\n")
            .map_err(|error| error.to_string())
    }

    /// Borrows document rows only long enough to copy neutral probe text.
    fn alphabet_rows(&self) -> Result<Vec<(u32, String, usize)>, String> {
        // Inspect the sealed rows used by the retained GPU image, never newer live VT.
        let mut rows = Vec::new();
        for index in 0..self.snapshot.rows() {
            let Some(row) = self.snapshot.retained_row(index) else {
                continue;
            };
            let mut text = String::new();
            for column in 0..self.snapshot.columns() {
                if let Some(cell) = cell_from_row(row, column) {
                    match cell.text {
                        CellText::Scalar(ch) => text.push(ch),
                        CellText::Cluster(cluster) => text.push_str(cluster),
                        CellText::Empty => text.push(' '),
                    }
                }
            }
            if let Some(column) =
                text.find("scroll ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz")
            {
                rows.push((index as u32, text[column..].to_owned(), column));
            }
        }
        if rows.is_empty() {
            return Err("authored alphabet row absent from sealed viewport".into());
        }
        Ok(rows)
    }
}

/// Counts non-paper pixels strictly inside a cell, independent of theme colors.
fn ink(image: &puzzle_render::RgbaImage, x: u32, y: u32, width: u32, height: u32) -> usize {
    // The neutral row has uniform paper; its top-left pixel is outside glyph ink.
    // Bounds-checked access makes clipped edge cells diagnostic failures rather
    // than panics if a resize lands between warm-up and the retained readback.
    let pixel = |x: u32, y: u32| {
        image
            .pixels
            .get(((y * image.width + x) * 4) as usize..((y * image.width + x) * 4 + 4) as usize)
    };
    let background = pixel(x, y);
    let mut ink = 0;
    // Exclude the cell border, where adjacent glyph overhang or cursor edges
    // could provide ink for a genuinely blank character.
    for py in y + 1..(y + height).min(image.height) {
        for px in x + 1..(x + width).min(image.width) {
            if pixel(px, py).is_some_and(|value| Some(value) != background) {
                ink += 1;
            }
        }
    }
    ink
}
