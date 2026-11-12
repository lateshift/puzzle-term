//! Host window creation followed by the safe embedding surface API (M17.embed).
//! The engine owns initial font metrics, PTY dimensions and worker startup.
//!
//! Winit retains ownership of native UI creation and observation. The engine
//! receives an Arc target and copied extent/display facts, so neither the
//! Session nor the renderer needs to query a Window from its worker thread.
//!
//! The creation stamp deliberately precedes create_window: first-ink timing
//! must include OS setup as well as GPU, font and child initialization. App
//! owns cancellation from the moment the surface is registered; InputWindow
//! keeps only the checked capability used by native event callbacks.
//!
//! Personal settings have already been validated before this callback. Their
//! clone is a cold per-surface snapshot, never a shared mutable store. The
//! optional recorder and authored workload observer retain their existing
//! diagnostic behavior while all product assembly stays inside puzzle-embed.
//! Failure propagates to resumed(), which logs it and exits the event loop.
//! No partially initialized input window is installed on that error path.
use super::{WindowApp, create_window};
use puzzle_embed::SurfaceOptions;
use puzzle_term::theme::Appearance;
use winit::event_loop::ActiveEventLoop;

impl WindowApp {
    /// Native UI setup contains no Session, GPU or font owner.
    pub(super) fn start(&mut self, loop_handle: &ActiveEventLoop) -> Result<(), String> {
        let settings = self
            .settings
            .take()
            .ok_or("settings already used by a window")?;
        let created = std::time::Instant::now();
        let window = create_window(loop_handle)?;
        let appearance = match window.theme() {
            Some(winit::window::Theme::Light) => Appearance::Light,
            _ => Appearance::Dark,
        };
        let mut options = SurfaceOptions {
            settings: Some(settings.clone()),
            appearance,
            window_created: Some(created),
            recorder: self.recorder.clone(),
            revision: self.recorder.as_ref().map(|_| super::record::revision()),
            #[cfg(feature = "lock-metrics")]
            observer: Some(Box::new(super::observer::Observer::new())),
            ..SurfaceOptions::default()
        };
        let granted = crate::producer::configure(self.producer.take(), &mut options)?;
        // Safe creation retains an Arc of the native view until render teardown.
        let (surface, perf) = self
            .engine
            .create_surface_with_diagnostics(
                window.clone(),
                super::host_surface::info(&window),
                super::host_surface::visibility(&window),
                options,
            )
            .map_err(|error| error.to_string())?;
        configure_surface(&surface, granted)?;
        // Retain the checked capability only after host services are configured.
        self.surface_id = Some(surface.id());
        // UI input forwards messages through the same Surface in either launch mode.
        self.live = Some(super::ui::InputWindow::new(
            window,
            surface,
            perf,
            settings,
            self.recorder.clone(),
        ));
        Ok(())
    }
}

/// Host services remain separate from approved producer selection and provisioning.
fn configure_surface(surface: &puzzle_embed::Surface, granted: bool) -> Result<(), String> {
    crate::producer::configure_surface(surface, granted)?;
    // Toasts use the shared overlay path; the unsigned harness needs no OS service.
    surface
        .notification_toasts()
        .map_err(|error| error.to_string())?;
    // The debug painter opts into observation through the checked host API.
    if std::env::var_os("PUZZLE_PERF_HUD").is_some() {
        surface
            .set_performance_hud(true)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}
