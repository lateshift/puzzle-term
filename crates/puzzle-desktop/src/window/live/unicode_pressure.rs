//! Complete-frame evidence combines actual submitted source handles and retained GPU pixels.
//!
//! Submitted coordinates were copied before the observer drained its builder
//! samples. Readback uses render-owned sealed frame data, without new layout.
//! Store inspection runs on the render owner with its existing instances.
//!
//! The authored frozen screen contains only system-covered Hangul syllables.
//! Every source placement must have final vector, color or raster ink; a
//! provisional hexadecimal border is deliberately rejected as unresolved.
//! Pixel crops then prove that those final handles reached the live GPU stores.
//! Background-only readback preserves resolved cell fills without text or cursors.
//! A solid full block differs from that background even when every corner is ink.
//! Shade cells also have uniform coverage, so their own corner cannot be paper.
//! Separate images use the same target format and executor's existing stores.
//! Both reads happen only in this optional observer, outside timed frame assembly.
//! Their overhead is retained in the complete flood's whole-interval denominator.
//! Glyph identities stay in the report to distinguish blank patterns from lost ink.
//! Neither font discovery nor a cold reference builder supplies substitute ink.
//! Uploads are temporarily omitted from readback to preserve this distinction.
//! The existing executor retains its native attachment format and atlas objects.
//! Its BGRA conversion happens after rendering and changes no shader behavior.
//! Grid scissors supply the device inset, avoiding assumptions about padding.
//! Integer cell metrics supply both wide glyph spans and the row's device height.
//! The entire per-cell list is retained, including successful and missing cells.
//! That prevents an aggregate zero from hiding an empty or incomplete witness.
//! Opt-in samples run after the timed redraw, including during the pressure
//! interval. Their readback overhead remains included in whole-interval fps.
//! Ordinary presentation and session-thread turns retain their existing work.
//!
//! Reviewer decision (2026-10-08): equal resolved foreground/background has
//! no observable RGB contrast. Such cells are checked by the placed source's
//! prepared coverage and counted separately; every other cell still needs pixels.
//! The equality fact was sealed alongside source coordinates at successful
//! presentation, so later palette/selection changes cannot waive this frame's test.
//! Unknown contrast defaults to the stricter pixel check. A missing or provisional
//! handle always fails, even if its intended ink would equal its background.
use super::LiveWindow;
use serde_json::{Value, json};
use std::path::Path;

/// Harness-only observation of the engine-owned surface.
pub(in crate::window) trait UnicodePressure {
    /// Runs the authored measurement using retained resources.
    fn frozen_snapshot(&self) -> bool;
    /// Preparation statistics come from the source that produced the image.
    fn glyph_timing(&self) -> Value;
    /// Inspect covered ink without preparing replacement glyphs.
    fn frozen_unicode(&mut self) -> Result<Value, String>;
    /// Ink and paper are independent reads of the same retained frame.
    fn retained_images(
        &mut self,
    ) -> Result<(puzzle_render::RgbaImage, puzzle_render::RgbaImage), String>;
}

impl UnicodePressure for LiveWindow {
    /// Classifies the submitted slot, even when newer title events already arrived.
    fn frozen_snapshot(&self) -> bool {
        self.snapshot.title() == "unicode-frozen"
    }
    /// Reads cumulative preparation counters without borrowing the render-owned fonts.
    fn glyph_timing(&self) -> Value {
        self.font_service
            .as_ref()
            .map_or(Value::Null, |service| service.timing_report())
    }

    /// Saves the unchanged Hangul screen and per-glyph ink/resolution evidence.
    fn frozen_unicode(&mut self) -> Result<Value, String> {
        let (image, paper) = self.retained_images()?;
        crate::png::write(Path::new("target/m11-unicode/frozen.png"), &image)?;
        let clip = self
            .frame
            .batches
            .iter()
            .find_map(|batch| batch.scissor)
            .ok_or("frozen grid clip absent")?;
        let metrics = self.text_context.metrics();
        // Each source owns its full cluster span, including wide-character spacers.
        let samples: Vec<_> = self
            .ink_observer
            .presented_cells
            .iter()
            .copied()
            .enumerate()
            // Final unsupported boxes are allowed; every eligible font source must resolve.
            .filter(|(_, cell)| cell[4] != puzzle_term::text::builtin::TOFU_FACE)
            .map(|(index, [row, col, span, ready, face, glyph])| {
                let x = clip.x + col * metrics.width_px;
                let y = clip.y + row * metrics.height_px;
                let count = ink(
                    &image,
                    &paper,
                    x,
                    y,
                    span * metrics.width_px,
                    metrics.height_px,
                );
                // Readiness alone may replace pixel evidence only for the reviewer's
                // explicit equal-color case, never for an arbitrary blank crop.
                let zero_contrast = self
                    .ink_observer
                    .presented_zero_contrast
                    .get(index)
                    .copied()
                    .unwrap_or(false);
                json!({"row":row,"column":col,"span":span,"resolved":ready == 1,"ink":count,
                    "zero_contrast":zero_contrast,
                    "face":face,"glyph":glyph})
            })
            .collect();
        Ok(coverage_report(
            samples,
            self.frame.target_size,
            self.text_context.dpr(),
        ))
    }

    /// Reads actual ink and its background independently; solid block cells have no empty corner.
    fn retained_images(
        &mut self,
    ) -> Result<(puzzle_render::RgbaImage, puzzle_render::RgbaImage), String> {
        // Re-uploading here could repair the atlas error this readback must catch.
        let uploads = std::mem::take(&mut self.frame.uploads);
        let image = self
            .frame_renderer
            .render_to_image(&self.renderer, &mut self.frame);
        let batches = std::mem::take(&mut self.frame.batches);
        // Retain actual resolved background/selection colors, including inverse-color cells.
        self.frame.batches.extend(
            batches
                .iter()
                .filter(|batch| batch.slot == puzzle_term::frame::ZSlot::Background)
                .cloned(),
        );
        let paper = self
            .frame_renderer
            .render_to_image(&self.renderer, &mut self.frame);
        // Restore both owners even if either GPU readback failed.
        self.frame.batches = batches;
        self.frame.uploads = uploads;
        Ok((
            image.map_err(|e| e.to_string())?.0,
            paper.map_err(|e| e.to_string())?.0,
        ))
    }
}

/// Covered equal-color cells require a final placed handle; all other cells require pixels too.
fn valid_ink(resolved: bool, zero_contrast: bool, pixels: u64) -> bool {
    // Keep preparation mandatory on both branches; visible ink additionally
    // proves that the retained handle reached the existing GPU texture stores.
    resolved && (zero_contrast || pixels > 0)
}

/// Counts glyph pixels in the complete cell span against independently drawn paper.
fn ink(
    image: &puzzle_render::RgbaImage,
    paper: &puzzle_render::RgbaImage,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
) -> usize {
    let pixel = |source: &puzzle_render::RgbaImage, px: u32, py: u32| {
        source
            .pixels
            .get(((py * image.width + px) * 4) as usize..((py * image.width + px) * 4 + 4) as usize)
            .map(|value| [value[0], value[1], value[2], value[3]])
    };
    // U+23B8 and box rules can place their entire stroke on a cell's first pixel.
    // The half-open crop includes that pixel without sampling the adjoining cell.
    (y..(y + h).min(image.height))
        .flat_map(|py| (x..(x + w).min(image.width)).map(move |px| (px, py)))
        .filter(|&(px, py)| {
            pixel(image, px, py).is_some_and(|value| Some(value) != pixel(paper, px, py))
        })
        .count()
}

/// Uniform block/shade coverage must count as ink; colored background alone must not.
#[cfg(test)]
mod tests;

/// Summarizes the exact submitted cells; an empty witness remains a failure.
fn coverage_report(samples: Vec<Value>, surface: [u32; 2], dpr: f32) -> Value {
    let missing = samples
        .iter()
        .filter(|c| {
            !valid_ink(
                c["resolved"] == true,
                c["zero_contrast"] == true,
                c["ink"].as_u64().unwrap_or(0),
            )
        })
        .count();
    // Coverage checks never waive preparation/placement; only invisible RGB ink is exempt.
    let coverage_checked = samples
        .iter()
        .filter(|c| c["zero_contrast"] == true)
        .count();
    // Empty evidence is a failure, never a vacuous all-covered result.
    json!({"cells":samples,"missing":missing,"pass":!samples.is_empty() && missing == 0,
        "coverage_checked":coverage_checked,"pixel_checked":samples.len()-coverage_checked,
        "surface_device_px":surface,"dpr":dpr})
}
