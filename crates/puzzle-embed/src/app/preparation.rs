//! Cold surface creation selects shared resources on their eventual render owner.
//!
//! Subsequent host input methods do not enter this handshake. A startup lease
//! guarantees that errors after font selection release the context on its owner.
use super::*;
use crate::render_thread::{
    resources::{Lease, Prepared},
    surface::SurfaceInfo,
};
impl App {
    /// Returns copied font geometry and a lifetime reservation, never a context borrow.
    pub(super) fn prepare(
        &self,
        surface: wgpu::Surface<'static>,
        info: SurfaceInfo,
        settings: &DesktopSettings,
    ) -> Result<(wgpu::Surface<'static>, Prepared, Lease), String> {
        // The actor receives an owned policy snapshot, never a borrow of the host store.
        let render_settings = settings.clone();
        // Instance handles are cheap clones; the actor selects the shared device once.
        let instance = self.instance.clone();
        // Completion may arrive from any source worker, so only enqueue host effects.
        let posts = self.posts.clone();
        // The supplied wakeup is the sole callback allowed outside the UI thread.
        let host_wake = Arc::clone(&self.config.wakeup);
        // A shared source completion is drawing work, not a new PTY output burst.
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            if posts.send(Posted::FallbackAll).is_ok() {
                host_wake();
            }
        });
        // This cold handshake transfers the native target, and returns it with copied metrics.
        // Normal input never uses this waiting path; it remains a Session control message.
        let (surface, prepared) = self.render.resources(move |resources| {
            resources
                .prepare(&instance, &surface, info, &render_settings, wake)
                .map(|prepared| (surface, prepared))
        })??;
        // Partial Session/PTY startup failure must not retain unused font resources.
        let lease = self.render.lease(prepared.key);
        Ok((surface, prepared, lease))
    }
}

impl App {
    /// Per-surface overrides are validated snapshots, independent of another surface.
    pub(super) fn surface_settings(&self, options: &super::SurfaceOptions) -> DesktopSettings {
        // Cloning occurs only at creation; worker input never borrows the host's store.
        // An absent override selects the same App-wide defaults as the first surface.
        options
            .settings
            .clone()
            .unwrap_or_else(|| self.config.settings.clone())
    }
}
