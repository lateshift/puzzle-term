//! Private action replay at the live frame-builder and GPU readback seam.
//!
//! Each recorded snapshot seals Session rows for a host-owned FrameBuilder,
//! which then builds and renders independently of the Session owner (D-50),
//! including the interval where parser output may arrive before presentation.
//! A fresh builder renders the same frozen snapshot for a pixel comparison.
//! All text and PNGs stay in ignored target/ because a user recording may
//! contain private file content. Only numeric frame results enter PROGRESS.
//!
//! The two renderers answer a narrow question: did a retained row cache or
//! an instance upload make a presented image disagree with its VT state?
//! A cold build starts with empty row and glyph caches, but uses the same
//! terminal, font bytes, device-pixel metrics, blink phase and focus state.
//! Equal pixels do not prove the transient VT state was desirable; they
//! locate its cause before drawing, in the parsed grid or frame schedule.
//! That distinction matters for nvim's show-command update: the first
//! synchronized group draws a control-key marker, and the next erases it.
//! Both groups are valid VT output; only presentation timing decides if
//! a human sees the short-lived marker.
//!
//! Snapshot capture happens before later parse entries, just as in T4's
//! native loop. The warm builder keeps damage-proportional row copies.
//! A cold builder cannot consume those copies, because unchanged rows are
//! deliberately absent. We therefore take a second, complete snapshot at
//! the same replay point. It shares VT state but has its own row buffers.
//! This avoids calling a cold result "different" merely because it lacks
//! cells that the warm builder already retained from a prior frame.
//!
//! The offscreen target is Rgba8Unorm, matching the harness's pixel format.
//! Rect and glyph positions use device pixels, with a 16-pixel grid inset.
//! Difference coordinates are converted back into row and column for
//! numerical reports. A mismatch image is diagnostic only: the private
//! recording can contain user text, so no image from this probe is staged.
//! Focus must be replayed alongside bytes. A focused block cursor is filled;
//! after focus loss the same style is a one-pixel hollow outline. Blink
//! phase likewise changes pixels without changing a VT cell or row version.
//!
//! D-30 moves fonts and the warm atlas out of Session. The replay host
//! keeps one mutable TextContext beside its GPU executor and lends it to
//! every sealed publication without borrowing Session. The cold
//! judge deliberately creates a separate context from the same approved
//! font bytes, so neither a cached shape nor a stale CPU atlas mirror can
//! approve its own wrong pixels. DPR changes reconfigure the warm context
//! before Session changes grid metrics, matching the native host's order.
//! An atlas reset increments a generation; Session copies every visible
//! row before a builder can use handles from that new incarnation.

use super::super::{drawn as harness_drawn, text};
use crate::{block_on::block_on, png};
use puzzle_render::{FrameRenderer, Renderer, RgbaImage};
use puzzle_term::text::FontData;
use puzzle_term::{
    Session,
    frame::RenderFrame,
    frame_builder::{FrameBuilder, context::TextContext},
    frame_snapshot::FrameSnapshot,
    text::font_set::FontSet,
};
use serde_json::Value as Json;
use std::{fs, path::Path, time::Instant};

/// GPU resources and font inputs reused across a private recording.
pub(super) struct DrawProbe {
    /// Device shared by persistent and cold renderers.
    renderer: Renderer,
    /// Persistent atlas and pipelines follow the live application path.
    gpu: FrameRenderer,
    /// Damage-proportional copy sealed before inspecting retained renderer caches.
    snapshot: FrameSnapshot,
    /// Permanent drawing caches belong to this replay host, never to Session.
    builder: Option<FrameBuilder>,
    /// Reused neutral instances for the incremental path.
    frame: RenderFrame,
    /// Reused font and atlas context for retained recorded frames.
    context: TextContext,
    /// Approved regular variable face supplied by the native harness.
    regular: FontData,
    /// Approved italic face needed for styled cells.
    italic: FontData,
    /// Approved symbol fallback for private icon cells.
    symbols: FontData,
    /// Logical font size stored in the recording header.
    font_size: f32,
    /// Display scale stored in the recording header.
    dpr: f32,
    /// A cold renderer must use the recorded window focus for cursor paint.
    focused: bool,
    /// Number of recorded snapshots checked so far.
    frames: usize,
    /// Number of frames whose incremental pixels differ from cold pixels.
    divergent: usize,
}

/// Pixel evidence from two builders at one recorded snapshot boundary.
struct ImagePair {
    /// Incremental frame with retained caches and GPU buffers.
    warm: RgbaImage,
    /// Fresh frame from a complete copy of the same VT state.
    cold: RgbaImage,
    /// Physical cell addresses containing at least one different RGBA pixel.
    differences: Vec<(u32, u32)>,
    /// Cursor and SGR-5 visibility sampled from the retained builder.
    phase: bool,
    /// Width and height of one terminal cell in device pixels.
    cell: (u32, u32),
}

impl DrawProbe {
    /// Validates recorded geometry before creating an offscreen GPU device.
    pub(super) fn new(header: &Json) -> Result<Self, String> {
        // Recordings arrive from a host callback stream, so validate
        // numeric font geometry before it reaches GPU allocation sizes.
        let font = header.get("font").ok_or("drawn replay lacks font")?;
        let font_size = font
            .get("size")
            .and_then(Json::as_f64)
            .ok_or("drawn replay lacks font size")? as f32;
        let dpr = header
            .get("dpr")
            .and_then(Json::as_f64)
            .ok_or("drawn replay lacks DPR")? as f32;
        if !font_size.is_finite()
            || !(6.0..=96.0).contains(&font_size)
            || !dpr.is_finite()
            || !(0.5..=4.0).contains(&dpr)
        {
            return Err("recorded font geometry outside replay bounds".into());
        }
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        // A separate offscreen device keeps this diagnostic independent
        // of the live window's swapchain and display-refresh phase.
        let renderer =
            block_on(Renderer::new(&instance, None)).map_err(|error| error.to_string())?;
        let gpu = FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
        let regular = text::read_font("assets/fonts/JetBrainsMono-Variable.ttf")?;
        let italic = text::read_font("assets/fonts/JetBrainsMono-Italic-Variable.ttf")?;
        let symbols = text::read_font("assets/fonts/SymbolsNerdFontMono-Regular.ttf")?;
        let context = TextContext::new(
            FontSet::new(regular.clone(), Some(italic.clone()), Some(symbols.clone()))?,
            font_size,
            dpr,
        )?;
        Ok(Self {
            renderer,
            gpu,
            builder: Some(FrameBuilder::new(&context)),
            snapshot: FrameSnapshot::default(),
            frame: RenderFrame::default(),
            context,
            regular,
            italic,
            symbols,
            font_size,
            dpr,
            focused: true,
            frames: 0,
            divergent: 0,
        })
    }

    /// Gives the replay Session the same font configuration as the window.
    pub(super) fn install(&self, session: &mut Session) {
        session.set_text_context(&self.context);
    }

    /// Reuses loaded font bytes while changing the recorded display scale.
    pub(super) fn set_dpr(&mut self, session: &mut Session, dpr: f32) -> Result<(), String> {
        self.context.reconfigure(self.font_size, dpr)?;
        self.dpr = dpr;
        session.set_text_context(&self.context);
        Ok(())
    }

    /// Mirrors host focus so hollow and filled cursors have the same cause.
    pub(super) fn set_focus(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Creates independent font caches from the same approved bytes.
    fn fonts(&self) -> Result<FontSet, String> {
        // Cold and retained builders must resolve glyph fallback from
        // identical bytes, or a font difference would look like cache loss.
        FontSet::new(
            self.regular.clone(),
            Some(self.italic.clone()),
            Some(self.symbols.clone()),
        )
    }

    /// Renders one exact recorded publication through the permanently owned cache.
    pub(super) fn capture(
        &mut self,
        session: &mut Session,
        number: u64,
        now: Instant,
        output: &Path,
    ) -> Result<(), String> {
        // Temporarily move our own cache so inspection can borrow the other host fields.
        // Session never receives it, including when capture or GPU readback fails.
        let mut builder = self.builder.take().ok_or("replay builder absent")?;
        // A recreated atlas invalidates old handles without discarding replay row identities.
        builder.sync_generation(self.context.atlas_generation());
        let outcome = session.publish_frame(
            now,
            &mut self.snapshot,
            self.context.atlas_generation(),
            builder.style_epoch(),
        );
        // Restore the diagnostic host’s cache on every failure path.
        let result = match outcome {
            Ok(puzzle_term::PublishOutcome::Published) => {
                self.inspect(session, &mut builder, number, output)
            }
            Ok(puzzle_term::PublishOutcome::NothingNew) => {
                Err(format!("frame {number} lacked a requested snapshot"))
            }
            Err(error) => Err(error),
        };
        self.builder = Some(builder);
        // Failed verification demands a complete recapture, preserving pending GPU uploads.
        if result.is_err() {
            session.apply(
                puzzle_term::SessionMessage::FrameResult(puzzle_term::FrameResult::BuildFailed),
                now,
            )?;
        }
        result
    }

    /// Compares pixels, then saves inferred text for every recorded frame.
    fn inspect(
        &mut self,
        session: &Session,
        builder: &mut FrameBuilder,
        number: u64,
        output: &Path,
    ) -> Result<(), String> {
        let ImagePair {
            warm,
            cold,
            differences,
            phase,
            cell,
        } = self.render_pair(session, builder)?;
        if !differences.is_empty() {
            // Only exceptional frames need image evidence. The recording
            // contains private text, so these PNGs stay in ignored target/.
            save_mismatch(output, number, &warm, &cold)?;
        }
        let cursor = self.snapshot.cursor().map(|cursor| {
            // Store coordinates and style only; inferred text belongs in
            // the private drawn file, not in a progress summary.
            (
                cursor.row_index,
                cursor.view.column,
                cursor.view.style,
                cursor.view.visible,
            )
        });
        self.frames += 1;
        self.divergent += usize::from(!differences.is_empty());
        let (drawn, notes) = harness_drawn::recognize::read_frozen(
            // Recognition is secondary to exact readback. Its uncertain
            // cells remain marked rather than fabricated as clean text.
            &self.renderer,
            session.terminal(),
            &warm,
            cold,
            cell,
            self.font_size,
            self.dpr,
        )?;
        fs::write(
            output.join(format!("replay-frame-{number:03}.drawn.txt")),
            drawn,
        )
        .map_err(|error| error.to_string())?;
        fs::write(
            output.join(format!("replay-frame-{number:03}.notes.txt")),
            format!(
                "cold_diff_cells={differences:?} cursor={cursor:?} phase={phase} focused={}\n{}\n",
                self.focused,
                notes.join("\n")
            ),
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Renders retained and fresh builders from the same terminal moment.
    fn render_pair(
        &mut self,
        session: &Session,
        builder: &mut FrameBuilder,
    ) -> Result<ImagePair, String> {
        builder.build_published(&mut self.context, &mut self.snapshot, &mut self.frame)?;
        // This renderer persists across all captured frames; stale GPU
        // buffers or row-cache uploads can therefore surface here.
        let (warm, _) = self
            .gpu
            .render_to_image(&self.renderer, &mut self.frame)
            .map_err(|error| error.to_string())?;
        let phase = builder.blink_phase();
        let cell = builder.metrics();
        // The warm builder's phase and geometry are sampled after assembly,
        // when they describe the pixels that were actually submitted.
        // A damage snapshot omits unchanged row cells. The new builder has
        // no row cache, so take a complete copy at this same replay instant.
        let mut full = FrameSnapshot::default();
        full.capture(session.terminal(), 0)?;
        let cold = self.cold_image(&full, phase, [warm.width, warm.height])?;
        let differences = differing_cells(&warm, &cold, cell.width_px, cell.height_px);
        // Exact RGBA comparison is appropriate here: both passes run on
        // the same GPU with the same shader and identical target format.
        Ok(ImagePair {
            warm,
            cold,
            differences,
            phase,
            cell: (cell.width_px, cell.height_px),
        })
    }

    /// Rebuilds all cells from the same snapshot and cursor blink phase.
    fn cold_image(
        &self,
        full: &FrameSnapshot,
        phase: bool,
        surface: [u32; 2],
    ) -> Result<RgbaImage, String> {
        let mut context = TextContext::new(self.fonts()?, self.font_size, self.dpr)?;
        let mut builder = FrameBuilder::new(&context);
        builder.set_blink_phase(phase);
        builder.set_focus(self.focused);
        // Focus is a host property, not part of VT. Omitting it would turn
        // a hollow warm cursor into a filled cold one after window blur.
        builder.set_surface_size(surface);
        let mut frame = RenderFrame::default();
        builder.build_snapshot(&mut context, full, &mut frame)?;
        let mut gpu = FrameRenderer::new(&self.renderer, wgpu::TextureFormat::Rgba8Unorm);
        gpu.render_to_image(&self.renderer, &mut frame)
            .map(|(image, _)| image)
            .map_err(|error| error.to_string())
    }

    /// Reports totals without exposing text from the private attachment.
    pub(super) fn totals(&self) -> (usize, usize) {
        (self.frames, self.divergent)
    }
}

/// Saves pixel evidence only under the private replay output directory.
fn save_mismatch(
    output: &Path,
    number: u64,
    warm: &RgbaImage,
    cold: &RgbaImage,
) -> Result<(), String> {
    // Both images have identical dimensions; separate names reveal whether
    // a missing cell belongs to retained state or to the fresh reference.
    png::write(
        &output.join(format!("replay-frame-{number:03}-warm.png")),
        warm,
    )?;
    png::write(
        &output.join(format!("replay-frame-{number:03}-cold.png")),
        cold,
    )
}

/// Projects byte disagreements onto the physical cell coordinates.
fn differing_cells(warm: &RgbaImage, cold: &RgbaImage, width: u32, height: u32) -> Vec<(u32, u32)> {
    let mut cells = std::collections::BTreeSet::new();
    for (index, (left, right)) in warm
        .pixels
        .chunks_exact(4)
        .zip(cold.pixels.chunks_exact(4))
        .enumerate()
    {
        if left == right {
            continue;
        }
        let x = index as u32 % warm.width;
        let y = index as u32 / warm.width;
        if x >= 16 && y >= 16 {
            cells.insert(((y - 16) / height, (x - 16) / width));
        }
    }
    cells.into_iter().collect()
}
