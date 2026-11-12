//! Main-thread OS observations and immediate input dispatch (D-41, D-43).
//!
//! Geometry and visibility cross the actor as copied data. The host applies
//! input and appearance as Surface messages, then requests paced damage.
use super::InputWindow;
use winit::event::WindowEvent;

/// Toolkit observations end here; every render update carries owned plain metadata.
impl InputWindow {
    /// Handles one callback without accessing a GPU, context or row builder.
    pub(in crate::window) fn handle_event(&mut self, event: WindowEvent) {
        let _stage = crate::resize_timing::Stage::new("main_callback");
        // Main-thread timestamps remain independent of long render-thread frame costs.
        let _busy = self.perf.callback(matches!(event, WindowEvent::Resized(_)));
        match event {
            WindowEvent::HoveredFile(path) => self.file_drag(path, false),
            WindowEvent::DroppedFile(path) => self.file_drag(path, true),
            WindowEvent::HoveredFileCancelled => self.drag_left(),
            // Record the observed physical extent before the worker can snapshot it.
            // Drag events may overwrite each other in the mailbox; the final extent wins.
            WindowEvent::Resized(size) => {
                self.record_event(
                    "resize",
                    serde_json::json!({"width":size.width,"height":size.height}),
                );
                // A display incarnation also distinguishes equal-rate monitor crossings.
                let info = self.surface_info();
                self.sent(self.session.resize(info));
            }
            // Scale and backing extent are coalesced into one actor update.
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.record_event("dpr", serde_json::json!({"value":scale_factor}));
                let mut info = self.surface_info();
                // The callback supplies the new scale even before a toolkit query catches up.
                info.scale = scale_factor;
                self.sent(self.session.resize(info));
            }
            // Moving inside one monitor preserves phase; only an identity change resets it.
            // All toolkit comparisons happen on the thread which owns the window.
            WindowEvent::Moved(_) => {
                // Probe zoom must follow the observed move, not its asynchronous request.
                #[cfg(feature = "lock-metrics")]
                super::super::measurement_display::maximize_arrived(&self.window);
                let info = self.surface_info();
                tracing::trace!(target:"puzzle_desktop::resize",event="desktop.host_moved",?info,
                    position=?self.window.outer_position(),outer=?self.window.outer_size());
                self.sent(self.session.resize(info));
                // Copied visibility observations accompany the new monitor clock.
                self.log_visibility("monitor_move");
            }
            // Theme policy remains an immediate typed host fact.
            WindowEvent::ThemeChanged(theme) => {
                // An absent/unknown preference elsewhere uses the documented dark fallback.
                let appearance = if theme == winit::window::Theme::Light {
                    puzzle_term::theme::Appearance::Light
                } else {
                    puzzle_term::theme::Appearance::Dark
                };
                // Theme transitions are typed Session policy, not GPU or font work.
                // The session owner applies this message independently of the render update.
                self.sent(self.session.appearance(appearance));
                self.sent(self.session.request_frame());
            }
            // Unknown visibility never becomes a fabricated visible observation.
            WindowEvent::Occluded(occluded) => {
                let mut visibility = super::super::host_surface::visibility(&self.window);
                // Preserve the actual boolean; unknown remains distinct before the first edge.
                visibility.occluded = Some(occluded);
                self.sent(self.session.visibility(visibility));
            }
            // Expose damage requests a paced render turn; it never presents on the host.
            WindowEvent::RedrawRequested => self.sent(self.session.request_frame()),
            // Queue input independently of damage; encoding runs on the Session owner.
            other => {
                if super::super::input::handle(self, other) {
                    self.sent(self.session.request_frame());
                }
            }
        }
    }
}
