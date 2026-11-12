//! Exclusive text/GPU ownership for the process render actor (D-30).
//!
//! Contexts are keyed by font policy, device geometry and glyph allowance.
//! A surface checks out its bundle for exactly one serialized driver turn;
//! row caches and snapshots stay with that surface. No mutex protects fonts.
//! Cold preparation returns copied metrics to the host before the PTY starts.
//! The lease keeps an unused context alive only until its last surface closes.
use crate::{settings::DesktopSettings, window::live::loan::Shared};
use puzzle_render::{ErrorMonitor, Renderer};
use puzzle_term::{FontMetrics, settings::SettingsValue};
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::AtomicBool},
};
/// First-target device selection, context preparation and surface configuration.
mod prepare;
/// Device replacement invalidates all contexts before any surface uses the new device.
mod recovery;

/// Identity of immutable rendering policy; window size and display ID are absent.
#[derive(PartialEq)]
struct Key {
    /// Scale determines bitmap geometry and hinted outlines, independent of display identity.
    scale: f32,
    /// Transient zoom changes the actual logical size without changing the stored setting.
    size: f32,
    /// Every schema font key participates, so new font settings cannot silently alias.
    font: Vec<(&'static str, SettingsValue)>,
    /// A host's configured allowance cannot silently affect another surface.
    budget: u32,
}
impl Key {
    /// Cold policy comparison allocates only on creation or a real settings/scale change.
    fn new(settings: &DesktopSettings, scale: f32) -> Self {
        let effective = settings.store.effective();
        Self {
            scale,
            size: settings.run_font_size(),
            font: puzzle_term::settings::Settings::schema()
                .into_iter()
                .filter(|def| def.key.starts_with("font."))
                .filter_map(|def| effective.value(def.key).map(|value| (def.key, value)))
                .collect(),
            budget: effective.render_glyph_memory_mb,
        }
    }
}
/// One text key remains exclusively owned even while lent to a surface driver.
struct Entry {
    /// Window geometry cannot invalidate this policy key.
    key: Key,
    /// The option is empty only during one non-reentrant actor turn.
    shared: Option<Shared>,
    /// Active surfaces and in-progress startup reservations retain the context.
    users: usize,
    /// Last attachment format selects shader handles, never a second font system.
    format: wgpu::TextureFormat,
    /// Source policy used to rebuild device-bound coverage after loss.
    settings: DesktopSettings,
}
/// Actor-local registry; hosts cannot borrow its GPU or text objects.
#[derive(Default)]
pub struct Resources {
    /// Device and queue are selected once from the first compatible native surface.
    gpu: Option<Gpu>,
    /// Unused entries are removed rather than retaining font systems indefinitely.
    entries: BTreeMap<u64, Entry>,
    /// Never reused, including after the final surface leaves a context.
    next: u64,
}
/// One device callback wakes all surfaces instead of being replaced by each window.
struct Gpu {
    /// Shared actual device/queue; Renderer clones never request another device.
    renderer: Renderer,
    /// The uncaptured-error callback is installed exactly once per device.
    errors: ErrorMonitor,
    /// Loss publication contains no borrowed native target.
    lost: Arc<AtomicBool>,
    /// Recovery wakes the host through its only permitted worker callback.
    wake: Arc<dyn Fn() + Send + Sync>,
    /// Shader state is compiled once per attachment format, shared by every context.
    pipelines: Vec<puzzle_render::FramePipelines>,
    /// Surfaces reconfigure when their last adopted incarnation differs.
    epoch: u64,
}
/// Copied measurements, never a text-context reference crossing into a Session.
#[derive(Clone, Copy)]
pub(crate) struct Prepared {
    /// Actor-local resource identity retained by a separately owned lease.
    pub key: u64,
    /// The initial PTY and Session use these exact rendering metrics.
    pub metrics: FontMetrics,
    /// Current device limit establishes exact image admission before child output.
    pub image_texture_limit: u32,
    /// Current device incarnation for per-surface configuration.
    pub epoch: u64,
    /// Attachment format selected from this native surface's capabilities.
    pub format: wgpu::TextureFormat,
    /// Existing native in-flight policy also bounds IOSurface pool storage.
    #[cfg(target_os = "macos")]
    pub latency: u32,
}
impl Resources {
    /// Returns an exclusive moving loan; a missing entry is a lifecycle error.
    pub(crate) fn take(&mut self, id: u64, format: wgpu::TextureFormat) -> Result<Shared, String> {
        let pipelines = self
            .gpu
            .as_mut()
            .ok_or("GPU unavailable")?
            .pipelines(format);
        let entry = self.entries.get_mut(&id).ok_or("text context is absent")?;
        entry.format = format;
        let shared = entry
            .shared
            .as_mut()
            .ok_or("text context is already borrowed")?;
        shared.frame_renderer.set_pipelines(pipelines);
        self.entries
            .get_mut(&id)
            .and_then(|entry| entry.shared.take())
            .ok_or_else(|| "text context is absent or already borrowed".into())
    }
    /// Returns the same bundle before another surface advances.
    pub(crate) fn put(&mut self, id: u64, shared: Shared) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.shared = Some(shared);
        }
    }
    /// Last-surface release frees its font service, atlases and GPU textures.
    pub(crate) fn release(&mut self, id: u64) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.users = entry.users.saturating_sub(1);
            if entry.users == 0 {
                self.entries.remove(&id);
            }
        }
    }
    /// Surface code reads an incarnation rather than comparing device pointers.
    pub(crate) fn epoch(&self) -> u64 {
        self.gpu.as_ref().map_or(0, |gpu| gpu.epoch)
    }
}
/// A startup reservation also releases on any partial PTY or surface failure.
pub(crate) struct Lease {
    /// The current key may change only at a serialized settings/scale boundary.
    pub key: u64,
    /// Destruction posts release to the actor that owns the registry.
    pub(super) commands: std::sync::mpsc::Sender<super::Command>,
    /// Startup failures wake an idle actor so their context is released promptly.
    pub(super) wake: std::sync::mpsc::SyncSender<()>,
}
impl Drop for Lease {
    /// No host teardown may directly destroy a text context on the wrong thread.
    fn drop(&mut self) {
        let key = self.key;
        let _ = self
            .commands
            .send(super::Command::Resources(Box::new(move |resources| {
                resources.release(key);
            })));
        let _ = self.wake.try_send(());
    }
}

#[cfg(test)]
/// Pixel equality, per-key policy isolation and all-context device recovery.
mod tests;
