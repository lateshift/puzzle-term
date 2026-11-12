//! Cold native/GPU attachment is separate from Session and child ownership.
//!
//! This value remains on main through registration. Its render-only pool moves
//! into Initial while the CALayer owner stays with App. Failed startup releases
//! both halves automatically; no callback can address a half-registered surface.
//!
//! Cold startup may wait for GPU/font selection before the child exists. That
//! handshake returns the native target to main so the CALayer subclass is never
//! initialized from the render worker. Normal input/resize methods do not use
//! this handshake; they keep their message-only contract.
//!
//! Registration is the ownership boundary. The driver receives the snapshot
//! receiver, reusable frame state and text reservation. App receives cancellation
//! owners, the Session event inbox and the native layer attachment. The returned
//! Surface capability sends messages and reads immutable presented pixels; it
//! contains no toolkit object or authority to mutate the renderer's resources.
//! Installing the geometry callback endpoint is the last step: until then its
//! OnceLock is empty and a premature native display request is inert.
use super::*;
use crate::{
    render_thread::resources::{Lease, Prepared},
    window::{driver::pooled::Initial, perf},
};
use std::time::Instant;

/// Cold resources are selected before a child can produce its first byte.
pub(super) struct Cold {
    /// Retained native target survives transfer to the rendering actor.
    surface: wgpu::Surface<'static>,
    /// Copied visibility/geometry and queue-only action sink have no toolkit borrow.
    host: crate::render_thread::host::HostView,
    /// Validated per-surface policy is independent of subsequent host-store changes.
    pub settings: DesktopSettings,
    /// Resource incarnation establishes font metrics and image admission limits.
    pub prepared: Prepared,
    /// Reservations are released even if later PTY/Session initialization fails.
    lease: Lease,
    /// Host creation time remains the first-frame budget origin.
    created: Instant,
    /// Cold configure completion remains separate from shell startup.
    configured: Instant,
    /// Main owns native layer mutation for the entire surface lifetime.
    #[cfg(target_os = "macos")]
    attachment: presentation::Attachment,
    /// Imported textures and completion ownership move to the sole render actor.
    #[cfg(target_os = "macos")]
    native: crate::window::live::presentation::contents::Native,
}
impl Cold {
    /// Child focus policy uses the same copied observations later lent to rendering.
    pub(super) fn visibility(&self) -> &crate::Visibility {
        &self.host.visibility
    }
    /// Performs only cold work, with no child or Session requiring cancellation.
    pub(super) fn new(
        app: &App,
        id: SurfaceId,
        factory: impl FnOnce(&wgpu::Instance) -> Result<wgpu::Surface<'static>, String>,
        info: crate::SurfaceInfo,
        visibility: crate::Visibility,
        options: &SurfaceOptions,
    ) -> Result<Self, ApiError> {
        // A surface receives a resolved settings snapshot, not a live host store.
        // Other panes can keep their own font/context keys while this one starts.
        let settings = app.surface_settings(options);
        // Creating on main satisfies the safe window/native-layer target contract.
        let surface = factory(&app.instance)?;
        let created = options.window_created.unwrap_or_else(Instant::now);
        // The actor selects shared GPU/text owners and returns only copied metrics.
        // The lease balances this reservation if any later native or PTY step fails.
        let (surface, prepared, lease) = app.prepare(surface, info, &settings)?;
        // UI attachment stays on main; only its bounded image pool transfers.
        #[cfg(target_os = "macos")]
        let (attachment, native) = presentation::Attachment::new(
            app,
            id,
            &surface,
            info,
            prepared.latency,
            presentation::initial_background(&settings, options.appearance)?,
        )?;
        #[cfg(not(target_os = "macos"))]
        let _ = id;
        let host = app.host_view(id, info, visibility);
        #[cfg(target_os = "macos")]
        let host = host.with_submission(attachment.submissions());
        Ok(Self {
            surface,
            host,
            settings,
            prepared,
            lease,
            created,
            configured: Instant::now(),
            #[cfg(target_os = "macos")]
            attachment,
            #[cfg(target_os = "macos")]
            native,
        })
    }
    /// Registration transfers all cancellation owners before publishing the endpoint.
    pub(super) fn finish(
        self,
        app: &mut App,
        id: SurfaceId,
        gutter_columns: u8,
        started: pty_bridge::Attached,
        pipeline: Pipeline,
        options: &mut SurfaceOptions,
    ) -> Result<(Surface, perf::Counters), ApiError> {
        // Initial owns the publication receiver exactly once. The Session sender
        // is cloned for rendering, while the host keeps its independent event inbox.
        let initial = Initial {
            host: self.host,
            gutter_columns,
            surface: self.surface,
            prepared: self.prepared,
            created: self.created,
            configured: self.configured,
            settings: self.settings,
            session: started.handle.clone(),
            frames: started.frames,
            recorder: options.recorder().cloned(),
            #[cfg(target_os = "macos")]
            presentation: Some(self.native),
        };
        // The pooled driver owns all render resources before registration. An
        // unsuccessful registration drops it and releases its reservation.
        let (driver, perf) = super::startup::driver(initial, self.lease, options);
        // All cancellation/join owners move together; failed registration cannot
        // leave a child running or a layer callback addressing an unused identity.
        let surface = app.register(
            id,
            Box::new(driver),
            started.handle,
            started.events,
            pipeline,
            started.provisioned,
        )?;
        #[cfg(target_os = "macos")]
        if let Some(entry) = app.surfaces.get_mut(&id) {
            self.attachment.install(entry, perf.clone());
        }
        Ok((surface, perf))
    }
}
