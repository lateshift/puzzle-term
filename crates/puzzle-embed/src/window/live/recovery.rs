//! Device-loss callbacks wake the owning render worker; recovery stays there (§10.1).
//!
//! No callback can borrow a surface or context. After loss the actor requests a
//! new device from its compatible adapter, rebuilds pipelines, and marks CPU
//! mirrors dirty. Reusable row caches then see the restored atlas generation.
use super::LiveWindow;
use crate::render_thread::host::HostView;
use puzzle_render::{FrameRenderer, Renderer};
use std::sync::atomic::Ordering;

/// Reuses the engine notifier while the harness driver is migrated.
pub use crate::render_thread::recovery::watch;

/// Device replacement is serialized with every frame and surface resize.
impl LiveWindow {
    /// Restores device-bound resources before any queued surface operation can execute.
    pub fn recover_device(&mut self) -> Result<bool, String> {
        if !self.device_lost.load(Ordering::Acquire) {
            return Ok(false);
        }
        // This wait runs only on the actor. Native input retains its independent control channel.
        let renderer =
            crate::block_on::block_on(self.renderer.recreate()).map_err(|e| e.to_string())?;
        #[cfg(target_os = "macos")]
        {
            // Device recreation must preserve the same unscaled stale-frame contract.
            // Reapply before configure: the replacement must never expose resize gravity.
            let reading = puzzle_sys::layer::set_surface_contents_top_left(&self.surface)?;
            tracing::debug!(event = "desktop.layer_gravity", ?reading);
        }
        // Install before configure/pipeline creation, whose validation can invoke wgpu inline.
        let gpu_errors = watch_errors(&renderer, self.window.clone());
        // Reuse host-supplied backing dimensions instead of querying a native window.
        let size = self.window.inner_size();
        crate::surface_config::configure(&renderer, &self.surface, size.width, size.height)?;
        // Pipelines must match the recreated device’s compatible surface format.
        let format = renderer
            .surface_format(&self.surface, size.width, size.height)
            .map_err(|e| e.to_string())?;
        // Device-bound textures and instance buffers cannot survive adapter reset.
        self.frame_renderer = FrameRenderer::new(&renderer, format);
        // Device loss changes resource identity, not the user's memory policy.
        // Fresh raster arrays must use the same allowance as restored CPU pages.
        self.frame_renderer
            .set_glyph_budget(&renderer, self.text_context.glyph_budget_bytes());
        // Optional readback also owns device-bound pipelines and must be replaced.
        self.verifier = super::verify::FrameVerifier::enabled(&renderer);
        // Watch the replacement before exposing it to any subsequent rendering turn.
        self.device_lost = watch(&renderer, self.window.clone());
        self.gpu_errors = gpu_errors;
        self.renderer = renderer;
        // Authored images/vectors restore from mirrors; grid coverage invalidates and rebuilds.
        self.text_context.reset_atlas();
        // Reproducible coverage uses the replacement device, without reading the lost device.
        crate::grid_rasterizer::install(
            &mut self.text_context,
            &self.renderer,
            &self.settings.store.effective().font_renderer,
        )?;
        // Row handles must be copied again after the restored atlas generation changes.
        self.snapshot.invalidate();
        // Old-device pending spans are obsolete; reset_atlas schedules complete mirrors.
        self.frame.uploads.clear();
        self.sync_font_metrics();
        tracing::info!(event = "desktop.device_restored");
        Ok(true)
    }
}

/// Product recovery preserves the previous image; diagnostic processes fail loudly.
pub fn watch_errors(renderer: &Renderer, host: HostView) -> puzzle_render::ErrorMonitor {
    // The callback sends only a wake; surface ownership remains on the render actor.
    puzzle_render::ErrorMonitor::install(renderer.device(), strict_errors(), move || {
        host.request_redraw()
    })
}

/// Diagnostic gates reject recovery instead of reporting a repaired frame as passing.
pub fn strict_errors() -> bool {
    // Probe gates must not certify a run that recovered from a validation defect.
    cfg!(test)
        || std::env::var_os("PUZZLE_GPU_FAIL_ON_ERROR").is_some()
        || std::env::var_os("PUZZLE_UNICODE_PROBE").is_some()
        || std::env::var_os("PUZZLE_SCROLL_PROBE").is_some()
}

impl LiveWindow {
    /// Consumes a callback failure outside every wgpu call, then forces a fresh snapshot.
    pub fn recover_gpu_error(&mut self) -> bool {
        // Consume exactly once after the callback has released its scalar-state guard.
        let Some(error) = self.gpu_errors.take_error() else {
            return false;
        };
        tracing::error!(event="desktop.frame_dropped", frame=self.next_frame, %error);
        #[cfg(feature = "lock-metrics")]
        self.frame_metrics.gate("validation_failed");
        // Both sides need invalidation: the local slot and the owner’s publication identity.
        // Otherwise an unchanged row could reuse the frame whose draw was rejected.
        self.snapshot.invalidate();
        self.send(puzzle_term::SessionMessage::FrameResult(
            puzzle_term::FrameResult::BuildFailed,
        ));
        // No drawable is held here. Reconfiguration cannot race an outstanding acquisition.
        let size = self.window.inner_size();
        // Minimized zero-sized surfaces wait for the next host geometry message.
        if size.width > 0 && size.height > 0 {
            self.resize_surface(size);
        }
        true
    }
}
