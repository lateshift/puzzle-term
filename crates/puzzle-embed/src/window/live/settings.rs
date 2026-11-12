//! Applies a validated config reload to the active native shell (§14.3).
//!
//! File parsing and validation occur on focus gain or an explicit binding,
//! never during the idle frame loop. The terminal keeps its grid content;
//! changed font metrics or padding require a reflow and matching PTY resize.
//! Font renderer policy is applied on the actor with other rendering settings.
//! A native Auto/Slug change invalidates cached rows even when font metrics agree.
//! Future cold masks bind the same device as the active surface executor.
use super::LiveWindow;

impl LiveWindow {
    /// Updates concrete subsystems from one resolved typed snapshot.
    pub fn apply_settings(&mut self, padding_changed: bool) {
        // Own this small cold-path copy before mutating the window fields;
        // the live parser and renderer read only the final typed snapshot.
        let effective = self.settings.store.effective().clone();
        // CPU/GPU owners switch together, never leaving old uploads under new handles.
        if let Err(error) = self.update_glyph_budget(effective.render_glyph_memory_mb) {
            tracing::warn!(event="settings.glyph_budget_rejected", %error);
            return;
        }
        let font_size = self.settings.run_font_size();
        let font_changed = match self.update_fonts(font_size, effective.font_fallback.as_slice()) {
            Ok(changed) => changed,
            Err(error) => {
                tracing::warn!(event="settings.font_rejected",%error);
                return;
            }
        };
        if let Err(error) = crate::grid_rasterizer::install(
            &mut self.text_context,
            &self.renderer,
            &effective.font_renderer,
        ) {
            tracing::warn!(event="settings.grid_renderer_rejected", %error);
            return;
        }
        self.sync_font_metrics();
        // Host input already applied bindings and policies before posting these settings.
        // Reapplying an older snapshot here could overwrite a newer host-side theme edit.
        if font_changed || padding_changed {
            // Font size or padding alters both visible columns and what the child
            // believes its PTY size is; update them from the same metrics.
            self.apply_grid_resize();
            self.flush_pty_resize();
        }
        // A cursor or color preference can change pixels without VT row
        // edits; request a frame after the style epoch has advanced.
        self.window.request_redraw();
        tracing::info!(event = "settings.reloaded", path = %self.settings.path().display());
    }
    /// Reinstalls metric-bound sources only when size or family order changes.
    fn update_fonts(&mut self, size: f32, families: &[String]) -> Result<bool, String> {
        let changed = self.text_context.font_size() != size || self.fallback_families != families;
        if changed {
            // Reconfiguration validates geometry before any old source is dropped.
            self.text_context
                .reconfigure(size, self.text_context.dpr())?;
            // New size-specific endpoints reuse the host-owned character-map index.
            // Old result lanes close with their context, cancelling obsolete sample delivery.
            // Completion continues through the same event proxy after a configuration reload.
            let font_wake = self.font_wake();
            crate::system_fonts::install_with_wake(
                &mut self.text_context,
                families,
                font_wake,
                &mut self.font_service,
            )?;
            self.fallback_families = families.to_vec();
        }
        Ok(changed)
    }
}

impl LiveWindow {
    /// Cold reload invalidates every row and releases oversized sampled textures.
    fn update_glyph_budget(&mut self, mib: u32) -> Result<(), String> {
        if self.text_context.set_glyph_memory_mb(mib)? {
            self.frame_renderer
                .set_glyph_budget(&self.renderer, self.text_context.glyph_budget_bytes());
            // Old atlas addresses and dirty spans name retired textures.
            self.frame.uploads.clear();
            self.snapshot.invalidate();
            self.sync_font_metrics();
        }
        Ok(())
    }
}
