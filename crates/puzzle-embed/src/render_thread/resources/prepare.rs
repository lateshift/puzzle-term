//! Cold native preparation, on the same owner as subsequent glyph work.
//!
//! The first surface loads fonts; a matching later surface only reads metrics.
//! Window dimensions and display identities do not enter the text key (D-30).
use super::*;
use crate::{block_on::block_on, render_thread::surface::SurfaceInfo};
use puzzle_render::FrameRenderer;
use puzzle_term::frame_builder::context::TextContext;
use std::sync::atomic::Ordering;

impl Resources {
    /// Selects the process device and configures this safe native target.
    pub(crate) fn prepare(
        &mut self,
        instance: &wgpu::Instance,
        surface: &wgpu::Surface<'static>,
        info: SurfaceInfo,
        settings: &DesktopSettings,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Prepared, String> {
        // A newly opened surface joins the current incarnation, never a lost device.
        self.recover()?;
        // First-target adapter selection respects native surface compatibility.
        if self.gpu.is_none() {
            let mut renderer =
                block_on(Renderer::new(instance, Some(surface))).map_err(|e| e.to_string())?;
            // Explicit evidence reads the real drawable; normal host surfaces stay write-only.
            if std::env::var_os("PUZZLE_SURFACE_COLOUR_REPORT").is_some() {
                renderer.enable_surface_readback();
            }
            // Later windows reuse this device/queue and their single notification owner.
            self.gpu = Some(Gpu::watch(renderer, wake, 1));
        }
        #[cfg(target_os = "macos")]
        {
            // Recreated targets obey D-54 before configure can expose stale frames.
            let reading = puzzle_sys::layer::set_surface_contents_top_left(surface)?;
            tracing::debug!(event = "desktop.layer_gravity", ?reading);
        }
        // A successful preparation establishes this invariant without a host-boundary panic.
        let gpu = self
            .gpu
            .as_ref()
            .ok_or("GPU preparation produced no device")?;
        // Configuration is per native target; fonts are not tied to its drawable size.
        // wgpu's Srgb enum clears the actual Metal profile. The native seam
        // restores named sRGB after configuration, including borrowed host layers.
        crate::surface_config::configure(
            &gpu.renderer,
            surface,
            info.extent.width,
            info.extent.height,
        )?;
        // Attachment compatibility chooses immutable shaders, separate from the font key.
        let format = gpu
            .renderer
            .surface_format(surface, info.extent.width, info.extent.height)
            .map_err(|e| e.to_string())?;
        // The reservation survives any later PTY startup failure through its actor lease.
        self.select(settings, info.scale as f32, format)
    }

    /// A key change selects a separate bundle; it cannot mutate another surface's fonts.
    pub(crate) fn select(
        &mut self,
        settings: &DesktopSettings,
        scale: f32,
        format: wgpu::TextureFormat,
    ) -> Result<Prepared, String> {
        // Key construction is cold and covers every schema font field plus transient zoom.
        let key = Key::new(settings, scale);
        if let Some((&id, entry)) = self.entries.iter_mut().find(|(_, entry)| entry.key == key) {
            // Retain before returning metrics so a preceding close cannot retire this context.
            entry.users += 1;
            return Self::prepared(
                id,
                entry,
                self.gpu.as_ref().ok_or("GPU unavailable")?.epoch,
                format,
            );
        }
        // A successful preparation establishes this invariant without a host-boundary panic.
        // A missing device is an initialization error, never permission to load fonts off-thread.
        let gpu = self.gpu.as_mut().ok_or("GPU unavailable")?;
        // A missing key constructs the full font/atlas bundle once on this owner.
        let shared = gpu.context(settings, scale, format)?;
        // Never reuse resource identities after the final user drops an old context.
        let id = self.next;
        self.next = self
            .next
            .checked_add(1)
            .ok_or("text context identities exhausted")?;
        // Keep source policy for all-context recovery without borrowing an active surface.
        let entry = Entry {
            key,
            shared: Some(shared),
            users: 1,
            settings: settings.clone(),
            format,
        };
        // Startup returns numeric geometry only; no mutable cache escapes the actor.
        let prepared = Self::prepared(id, &entry, gpu.epoch, format)?;
        // Publish only a fully initialized bundle, so a key hit can borrow immediately.
        self.entries.insert(id, entry);
        tracing::debug!(
            event = "embed.text_context_created",
            key = id,
            contexts = self.entries.len()
        );
        Ok(prepared)
    }
    /// Metrics are copied before any child output can race startup.
    ///
    /// The host uses this value to establish the initial PTY grid, and sends the
    /// same value to the Session. Neither path needs a text-context reference.
    /// Atlas generation is included because matching numeric cell sizes alone
    /// cannot prove that retained row handles name the current glyph addresses.
    /// The attachment format is supplied separately: two windows may use the
    /// same fonts and atlases while drawing to different compatible formats.
    /// Device epoch is process-wide, so an occluded surface can notice recovery
    /// on its first subsequent turn without consulting another surface owner.
    /// A missing bundle indicates a reentrant resource operation; initialization
    /// fails explicitly instead of lending aliased mutable glyph-cache state.
    fn prepared(
        id: u64,
        entry: &Entry,
        epoch: u64,
        format: wgpu::TextureFormat,
    ) -> Result<Prepared, String> {
        // Selection runs between turns, while all preceding moving loans have returned.
        let context = &entry
            .shared
            .as_ref()
            .ok_or("text context is currently borrowed")?
            .text_context;
        // FontMetrics is a copyable message payload for the Session owner.
        Ok(Prepared {
            key: id,
            // Copy the existing queue policy into native presentation rather than
            // inventing another pool depth when the host attaches its CA child.
            #[cfg(target_os = "macos")]
            latency: entry
                .shared
                .as_ref()
                .ok_or("text context is currently borrowed")?
                .renderer
                .maximum_frame_latency(),
            // Admission needs the current physical device limit, not a font policy
            // constant: tile halos must fit before the child receives protocol OK.
            image_texture_limit: entry
                .shared
                .as_ref()
                .ok_or("text context is currently borrowed")?
                .renderer
                .device()
                .limits()
                .max_texture_dimension_2d,
            epoch,
            format,
            metrics: FontMetrics {
                cell: context.metrics(),
                font_size: context.font_size(),
                dpr: context.dpr(),
                atlas_generation: context.atlas_generation(),
            },
        })
    }
}
impl Gpu {
    /// Exactly one loss/error notifier belongs to the process device, not a surface.
    pub(super) fn watch(renderer: Renderer, wake: Arc<dyn Fn() + Send + Sync>, epoch: u64) -> Self {
        // The callback owns scalar evidence and a wakeup, never a target or text borrow.
        let lost = Arc::new(AtomicBool::new(false));
        let notified = Arc::clone(&lost);
        let loss_wake = Arc::clone(&wake);
        // wgpu permits exactly one loss callback for the process device.
        renderer
            .device()
            .set_device_lost_callback(move |reason, message| {
                tracing::warn!(event="desktop.device_lost", ?reason, %message);
                // Release pairs with the actor’s Acquire before replacing GPU resources.
                notified.store(true, Ordering::Release);
                loss_wake();
            });
        let error_wake = Arc::clone(&wake);
        // Tests fail on validation; product surfaces retain the image and recover explicitly.
        let errors = ErrorMonitor::install(
            renderer.device(),
            crate::window::live::recovery::strict_errors(),
            move || error_wake(),
        );
        Self {
            renderer,
            errors,
            lost,
            wake,
            epoch,
            pipelines: Vec::new(),
        }
    }
}

impl Gpu {
    /// Attachment formats select immutable pipelines; fonts and sampled atlases stay shared.
    pub(super) fn pipelines(
        &mut self,
        format: wgpu::TextureFormat,
    ) -> &puzzle_render::FramePipelines {
        // Format diversity is small and cold; lookup never traverses glyph or row contents.
        let index = if let Some(index) = self.pipelines.iter().position(|p| p.format() == format) {
            index
        } else {
            // Compile each format once, rather than once per text key or window.
            self.pipelines
                .push(puzzle_render::FramePipelines::new(&self.renderer, format));
            self.pipelines.len() - 1
        };
        // Returning a shared shader reference lends no mutable atlas state.
        &self.pipelines[index]
    }
}

impl Gpu {
    /// Builds one missing font-policy bundle, independent of native window dimensions.
    ///
    /// Loading source bytes, choosing native fallback endpoints, compiling the grid
    /// coverage producer and allocating sampled textures are cold work. Keeping them
    /// together ensures a context cannot accidentally bind sources to another device.
    /// The Session never receives this object: only the copied metrics from prepared.
    /// A later matching surface selects the entry without executing this constructor.
    ///
    /// Source policy is immutable for an entry. A settings reload selects a new key
    /// before releasing the old one, so a neighbouring surface keeps its own fonts.
    /// A DPR change similarly gets new device-sized coverage while a window-size or
    /// equal-DPR display change retains all cache identity and uploaded atlas contents.
    ///
    /// Pipelines are device/format data, while sampled textures and shaping caches
    /// are text-key data. Their split avoids recompiling shaders for another font.
    /// Instance buffers are scratch used by serialized turns and do not need one
    /// allocation per surface; row-instance caches still remain per terminal.
    ///
    /// Errors propagate before insertion, so a partial load cannot become a key hit.
    /// Dropping a partial bundle also drops its discovery endpoints and cancels any
    /// obsolete source result lanes, without handing cleanup to the host UI thread.
    fn context(
        &mut self,
        settings: &DesktopSettings,
        scale: f32,
        format: wgpu::TextureFormat,
    ) -> Result<Shared, String> {
        // Loading and parsing bundled font bytes happens only for a missing key.
        let fonts = crate::font_data::bundled_font_set()?;
        let mut context = TextContext::new(fonts, settings.run_font_size(), scale)?;
        // Apply capacity before installing producers that can create the first atlas pages.
        context.set_glyph_memory_mb(settings.store.effective().render_glyph_memory_mb)?;
        // Any completion affects all surfaces sharing this source policy.
        let font_wake = Arc::clone(&self.wake);
        let mut font_service = None;
        // Native source endpoints are bounded; the service outlives each individual window.
        crate::system_fonts::install_with_wake(
            &mut context,
            settings.store.effective().font_fallback.as_slice(),
            Arc::clone(&font_wake),
            &mut font_service,
        )?;
        // Generated coverage writes directly to this shared device, without CPU mirrors.
        crate::grid_rasterizer::install(
            &mut context,
            &self.renderer,
            &settings.store.effective().font_renderer,
        )?;
        // Another font key uses the same compiled shaders but independent sampled textures.
        let pipelines = self.pipelines(format).clone();
        let mut frame_renderer = FrameRenderer::with_pipelines(&self.renderer, &pipelines);
        // CPU and GPU raster capacities always name the same validated allowance.
        frame_renderer.set_glyph_budget(&self.renderer, context.glyph_budget_bytes());
        // These owners always travel together during a surface turn.
        Ok(Shared {
            font_service,
            font_wake,
            renderer: self.renderer.clone(),
            device_lost: Arc::clone(&self.lost),
            gpu_errors: self.errors.clone(),
            frame_renderer,
            text_context: context,
            fallback_families: settings.store.effective().font_fallback.to_vec(),
        })
    }
}
