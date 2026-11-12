//! Opt-in cold GPU redraw of each live snapshot (§13.4).
//!
//! The reference and warm copies share the same sealed Session publication.
//! Two independent Rgba8Unorm executors then draw those copies;
//! this removes swapchain format and compositor behavior from the equality.
//! The warm executor persists atlas uploads across frames, while the cold
//! builder and executor begin empty every time. Only mismatches write PNGs.
use crate::{font_data, png};
use puzzle_render::{FrameRenderer, Renderer, RgbaImage};
use puzzle_term::{frame::RenderFrame, frame_builder::FrameBuilder, frame_snapshot::FrameSnapshot};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// Separates a genuine pixel disagreement from a failed diagnostic pass.
pub enum VerifyResult {
    /// Both independently rendered images have identical RGBA bytes.
    Match,
    /// The saved evidence names cells whose pixels differ.
    Mismatch(String),
}

/// Presentation state captured with one frame for an equivalent cold draw.
#[derive(Clone, Copy)]
pub struct VerifyRequest {
    /// Blink phase used for the warm cursor and text decorations.
    pub blink: bool,
    /// Focus selects a hollow rather than filled inactive cursor.
    pub focused: bool,
    /// Physical scale selects hinted or unhinted outlines.
    pub dpr: f32,
    /// Logical font size may differ after a config reload.
    pub font_size: f32,
    /// Presented frame ordinal used to name private evidence.
    pub number: u64,
}

/// Keeps slow readback away from active frame deadlines.
#[derive(Default)]
pub(super) struct VerifySchedule {
    /// An unchanged image may be checked after this quiet deadline.
    due: Option<Instant>,
    /// Every replaced image is counted instead of silently ignored.
    skipped: u64,
}

impl VerifySchedule {
    /// Queues a completed presentation for a later quiet turn.
    pub(super) fn presented(&mut self, now: Instant) {
        self.due = Some(now + Duration::from_millis(250));
    }

    /// Replaces stale evidence when a new paced frame must be drawn.
    pub(super) fn frame_started(&mut self) {
        if self.due.take().is_some() {
            self.skipped += 1;
        }
    }

    /// The host never checks while a terminal presentation is pending.
    pub(super) fn ready(&self, now: Instant, next_frame: Option<Instant>) -> bool {
        self.due.is_some_and(|due| now >= due) && next_frame.is_none()
    }

    /// Returns the OS idle wake deadline without polling.
    pub(super) fn due(&self) -> Option<Instant> {
        self.due
    }

    /// Consumes the check and its skipped-frame count for one log record.
    pub(super) fn finish(&mut self) -> u64 {
        self.due = None;
        std::mem::take(&mut self.skipped)
    }
}

/// Persistent GPU state for the incremental side of the comparison.
pub struct FrameVerifier {
    /// Retains texture uploads omitted from later incremental frames.
    warm: FrameRenderer,
}

impl FrameVerifier {
    /// Saves the burst's retained warm geometry after its timed interval ends.
    /// The offscreen format matches these diagnostic pipelines, while its atlas
    /// has followed every real surface upload since window creation.
    #[cfg(feature = "lock-metrics")]
    pub fn save_present_probe(
        &mut self,
        renderer: &Renderer,
        frame: &mut RenderFrame,
    ) -> Result<(), String> {
        // Mapping here cannot pace the earlier 300 surface submissions.
        let (image, _) = self
            .warm
            .render_to_image(renderer, frame)
            .map_err(|error| error.to_string())?;
        png::write(Path::new("target/m8-fast-present.png"), &image)
    }

    /// Creates the debug executor only when the environment flag is set.
    pub fn enabled(renderer: &Renderer) -> Option<Self> {
        // Environment opt-in is checked at window creation, so a normal
        // session never creates the extra pipelines or readback textures.
        // The spelling is exact to keep an accidental nonempty value from
        // turning on a very expensive GPU diagnostic during normal use.
        (std::env::var_os("PUZZLE_VERIFY_FRAMES").as_deref() == Some(std::ffi::OsStr::new("1")))
            .then(|| Self {
                warm: FrameRenderer::new(renderer, wgpu::TextureFormat::Rgba8Unorm),
            })
    }

    /// Keeps the offscreen atlas equal to the presented renderer's atlas.
    pub fn observe_uploads(
        &mut self,
        renderer: &Renderer,
        frame: &RenderFrame,
    ) -> Result<(), String> {
        // A frame may be replaced before the quiet readback begins, but its
        // newly prepared glyphs can still be reused by later frames.
        self.warm
            .apply_uploads(renderer, &frame.uploads)
            .map_err(|error| error.to_string())
    }

    /// Compares one complete presented state and preserves evidence on error.
    pub fn check(
        &mut self,
        renderer: &Renderer,
        warm: &mut RenderFrame,
        snapshot: &FrameSnapshot,
        request: VerifyRequest,
        families: &[String],
    ) -> Result<VerifyResult, String> {
        // Drawing the warm frame into RGBA8 uses its retained atlas and
        // instance arrays exactly as the just-presented surface did.
        // The swapchain itself cannot be read back on all native backends.
        let (warm_image, _) = self
            .warm
            .render_to_image(renderer, warm)
            .map_err(|error| error.to_string())?;
        // Reopening platform-owned fonts creates independent shaping and
        // glyph caches; a stale warm atlas cannot contaminate this judge.
        let mut cold_context = puzzle_term::frame_builder::context::TextContext::new(
            font_data::bundled_font_set()?,
            request.font_size,
            request.dpr,
        )?;
        // Live verification must use the same family chain as the presented window.
        crate::system_fonts::install(&mut cold_context, families)?;
        let mut cold_builder = FrameBuilder::new(&cold_context);
        // Both paths need the same physical target extent and cursor state.
        // A focused block and an unfocused hollow cursor differ even when
        // the underlying terminal cell bytes are identical.
        cold_builder.set_surface_size(warm.target_size);
        cold_builder.set_blink_phase(request.blink);
        cold_builder.set_focus(request.focused);
        let mut cold_frame = RenderFrame::default();
        // The sealed verification copy contains all visible rows, so
        // the new builder does not need Session's incremental row caches.
        crate::system_fonts::settle::snapshot(
            &mut cold_context,
            &mut cold_builder,
            snapshot,
            &mut cold_frame,
        )?;
        // A second executor starts with empty GPU atlases; the fresh frame
        // carries all uploads required for the same visible glyphs.
        let mut cold_renderer = FrameRenderer::new(renderer, wgpu::TextureFormat::Rgba8Unorm);
        let (cold_image, _) = cold_renderer
            .render_to_image(renderer, &mut cold_frame)
            .map_err(|error| error.to_string())?;
        if warm_image.pixels == cold_image.pixels {
            // Equality is byte-for-byte after rendering to the same format.
            // No tolerance can hide a lost row, cursor or atlas update.
            return Ok(VerifyResult::Match);
        }
        // The PNG trio is written only on disagreement, preserving the
        // exact pixels that produced the logged cell list for later review.
        let evidence = Self::save_difference(
            &warm_image,
            &cold_image,
            cold_builder.metrics().width_px,
            cold_builder.metrics().height_px,
            request.number,
            Path::new("target/verify"),
        )?;
        Ok(VerifyResult::Mismatch(evidence))
    }

    /// Writes a magenta difference and names each changed terminal cell.
    fn save_difference(
        warm: &RgbaImage,
        cold: &RgbaImage,
        cell_w: u32,
        cell_h: u32,
        number: u64,
        dir: &Path,
    ) -> Result<String, String> {
        // The relative path stays in ignored target/ during a private run.
        // Neither terminal contents nor these screenshots enter fixtures.
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let mut difference = RgbaImage {
            width: warm.width,
            height: warm.height,
            pixels: vec![0; warm.pixels.len()],
        };
        let mut cells = BTreeSet::new();
        // RGBA chunks compare a pixel as a unit. Index is a pixel offset,
        // so multiplying it by four identifies its first output channel.
        for (index, (left, right)) in warm
            .pixels
            .chunks_exact(4)
            .zip(cold.pixels.chunks_exact(4))
            .enumerate()
        {
            if left == right {
                continue;
            }
            // Report device pixels as row:column cells using the same
            // sixteen-pixel border and metrics as FrameBuilder's geometry.
            let x = index as u32 % warm.width;
            let y = index as u32 / warm.width;
            if x >= 16 && y >= 16 {
                // Border disagreements still color the diff but have no
                // terminal-cell location, so they stay out of this set.
                cells.insert(((y - 16) / cell_h, (x - 16) / cell_w));
            }
            // Magenta remains legible over both dark and light themes;
            // unchanged pixels are black to make sparse errors obvious.
            difference.pixels[index * 4..index * 4 + 4].copy_from_slice(&[255, 0, 255, 255]);
        }
        // The frame ordinal ties three images to one private recording
        // entry without putting any displayed text into the log record.
        let prefix = format!("frame-{number:06}");
        let warm_path = dir.join(format!("{prefix}-warm.png"));
        let cold_path = dir.join(format!("{prefix}-cold.png"));
        let diff_path = dir.join(format!("{prefix}-difference.png"));
        png::write(&warm_path, warm)?;
        png::write(&cold_path, cold)?;
        png::write(&diff_path, &difference)?;
        // A caller logs this as desktop.frame_mismatch while continuing
        // the live session so one glitch does not destroy later evidence.
        Ok(format!(
            "cells={cells:?} warm={} cold={} difference={}",
            warm_path.display(),
            cold_path.display(),
            diff_path.display()
        ))
    }
}

#[cfg(test)]
mod tests;
