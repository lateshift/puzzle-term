//! Shared-device recovery precedes every surface turn (D-30, §10.1).
//!
//! CPU source mirrors survive; generated grid masks are rebuilt on demand.
//! Each surface observes the new epoch and reconfigures its own drawable.
use super::*;
use std::sync::atomic::Ordering;
impl Resources {
    /// Replaces the device once, then resets every retained text context's GPU incarnation.
    pub(crate) fn recover(&mut self) -> Result<(), String> {
        // An App without surfaces has not selected an adapter and needs no recovery.
        let Some(gpu) = &self.gpu else {
            return Ok(());
        };
        // The callback publishes only this flag; all replacement work stays on the actor.
        if !gpu.lost.load(Ordering::Acquire) {
            return Ok(());
        }
        // Reuse the compatible adapter instead of creating a device for every surface.
        // Cold recovery may wait here, while Session input keeps its separate inbox.
        let renderer =
            crate::block_on::block_on(gpu.renderer.recreate()).map_err(|e| e.to_string())?;
        // Install notifications before compiling, since validation can call back inline.
        let mut replacement =
            Gpu::watch(renderer, Arc::clone(&gpu.wake), gpu.epoch.wrapping_add(1));
        // Every retained key shares the failed device, regardless of its current visibility.
        for entry in self.entries.values_mut() {
            // No loan can overlap this entry point: the actor invokes it before a driver turn.
            let shared = entry
                .shared
                .as_mut()
                .ok_or("device recovery during a text loan")?;
            // Shader handles share the new device and preserve each attachment format.
            let pipelines = replacement.pipelines(entry.format).clone();
            shared.frame_renderer =
                puzzle_render::FrameRenderer::with_pipelines(&replacement.renderer, &pipelines);
            // Replacement textures obey the existing configured limit, without a larger default.
            shared.frame_renderer.set_glyph_budget(
                &replacement.renderer,
                shared.text_context.glyph_budget_bytes(),
            );
            // Replace callback evidence along with GPU ownership, never leaving a stale flag.
            shared.renderer = replacement.renderer.clone();
            shared.gpu_errors = replacement.errors.clone();
            shared.device_lost = Arc::clone(&replacement.lost);
            // Image-only glyphs restore from CPU mirrors; outline masks rerasterize.
            shared.text_context.reset_atlas();
            // The reproducible grid producer must target the replacement queue and device.
            crate::grid_rasterizer::install(
                &mut shared.text_context,
                &replacement.renderer,
                &entry.settings.store.effective().font_renderer,
            )?;
        }
        // Only publish the new process incarnation after every context has been rebuilt.
        self.gpu = Some(replacement);
        tracing::info!(
            event = "desktop.device_restored",
            contexts = self.entries.len()
        );
        Ok(())
    }
}
