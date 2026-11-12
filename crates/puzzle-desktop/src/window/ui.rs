//! Native-thread input and window lifetime, independent of GPU work (D-41).
//!
//! Keyboard, IME, button and clipboard commands move to the Session owner
//! without a synchronous query or parser wait. The render worker owns every GPU object
//! and glyph context; this owner can only forward geometry/settings updates.
//!
//! The child's killer stays here so close releases blocked PTY operations
//! immediately. A safe surface may retain the native window until the actor
//! finishes an in-flight frame, without delaying this native callback.
//! Authoritative settings stay beside input: a font change publishes a cold
//! clone while Option policy and keybindings apply before the next key.
//! Monitor handles are compared only on the host; the actor sees a plain
//! display incarnation and may reset its clock even at an unchanged rate.
//! Pointer callbacks have singular provider ownership. Samples arriving
//! during a row build retain their input order independently of GPU completion.
//! macOS forwards winit's CG display ID so subscriptions follow the actual display.
//! Windows/Linux retain their incarnation counter and predicted-refresh timer.
//! No native monitor query occurs on the renderer or the link callback thread.
use crate::{recording::Recorder, settings::DesktopSettings};
use std::{sync::Arc, time::Instant};
use winit::window::Window;
/// Winit offers a bounded file-only subset of the full native drag contract.
mod dnd;
/// Owned effects run only on this native owner, outside Session policy.
pub(super) mod effects;
/// Geometry, appearance and visibility callbacks stay on the host.
mod events;
/// Stateful recordable key/IME adapter and immediate ordered writes.
mod input;
/// Provider input becomes an owner-side message against presented regions.
mod overlay_pointer;
/// Physical pointer samples are coalesced at display boundaries.
mod pointer;
/// File/config actions run on the host and forward rendering preferences.
mod settings;

/// The host-owned window and input adapters; contains no rendering resources.
pub(super) struct InputWindow {
    /// Native window operations belong to its event-loop thread.
    pub(super) window: Arc<Window>,
    /// Sending capability reaches a control inbox, never terminal state.
    pub(super) session: puzzle_embed::Surface,
    /// Geometry observations are hints only; owner-side policy resolves every pointer.
    input_state: Option<puzzle_term::session::host_state::InputState>,
    /// Shared scalars measure time inside our own callbacks, never event gaps.
    perf: super::perf::Counters,
    /// Last-good user/profile/session settings are authoritative on the host.
    settings: DesktopSettings,
    /// Optional registration allocates only after OSC 72 enables drops.
    drop_target: Option<Box<dnd::Target>>,
    /// Latest pointer point is view-local logical, including the rendered inset.
    drop_position: [f64; 2],
    /// Latest OS modifiers are used only for host-side pointer coalescing.
    modifiers: u8,
    /// Latest physical pointer sample and click-chain state.
    pointer: crate::mouse::Adapter,
    /// Monotonic click timing is independent of wall-clock changes.
    pointer_epoch: Instant,
    /// Optional private capture shares the parser/writer recorder.
    recorder: Option<Recorder>,
    /// Native monitor identity is compared here, never on the render worker.
    #[cfg(not(target_os = "macos"))]
    monitor: Option<winit::monitor::MonitorHandle>,
    /// A monitor crossing invalidates phase even when refresh rates match.
    #[cfg(not(target_os = "macos"))]
    display_epoch: u64,
    #[cfg(feature = "lock-metrics")]
    /// Diagnostic-only restore geometry and refresh-stepped drag positions.
    pub(in crate::window) magnet: super::lifecycle_probe::magnet::Replay,
}

impl InputWindow {
    /// An App wake can perform CA assignment outside a window-event callback.
    pub(in crate::window) fn callback(&self) -> super::perf::Callback {
        self.perf.callback(false)
    }
    /// Joins input services after ownership of the surface has moved to the actor.
    #[allow(
        clippy::too_many_arguments,
        reason = "input startup transfers distinct native owners once"
    )]
    pub(super) fn new(
        window: Arc<Window>,
        session: puzzle_embed::Surface,
        perf: super::perf::Counters,
        settings: DesktopSettings,
        recorder: Option<Recorder>,
    ) -> Self {
        #[cfg(not(target_os = "macos"))]
        let monitor = window.current_monitor();
        Self {
            window,
            session,
            input_state: None,
            drop_target: None,
            drop_position: [0.0; 2],
            perf,
            settings,
            modifiers: 0,
            pointer: crate::mouse::Adapter::default(),
            pointer_epoch: Instant::now(),
            recorder,
            // Restore and drag state is never allocated by ordinary product windows.
            #[cfg(feature = "lock-metrics")]
            magnet: Default::default(),
            #[cfg(not(target_os = "macos"))]
            monitor,
            #[cfg(not(target_os = "macos"))]
            display_epoch: 0,
        }
    }

    /// Copies the latest extent and names an actual monitor crossing.
    pub(super) fn surface_info(&mut self) -> puzzle_embed::SurfaceInfo {
        let _stage = crate::resize_timing::Stage::new("host_geometry_query");
        #[cfg(not(target_os = "macos"))]
        {
            let monitor = self.window.current_monitor();
            if self.monitor != monitor {
                self.display_epoch = self.display_epoch.wrapping_add(1);
                self.monitor = monitor;
            }
            let mut info = super::host_surface::info(&self.window);
            info.display = self.display_epoch;
            info
        }
        #[cfg(target_os = "macos")]
        super::host_surface::info(&self.window)
    }

    /// Preserves host geometry/focus facts beside raw input without entering the renderer.
    pub(super) fn record_event(&self, kind: &str, data: serde_json::Value) {
        if let Some(recorder) = &self.recorder
            && let Err(error) = recorder.emit(kind, data)
        {
            tracing::warn!(event = "desktop.record_failed", %error);
        }
    }

    /// Records visibility on the native thread and forwards the latest observations.
    pub(super) fn log_visibility(&self, source: &'static str) {
        tracing::debug!(event = "desktop.host_visibility", source,
            focused = self.window.has_focus(), visible = ?self.window.is_visible());
        self.sent(
            self.session
                .visibility(super::host_surface::visibility(&self.window)),
        );
    }
}

/// Native callbacks report checked API errors without gaining an owner endpoint.
impl InputWindow {
    /// Error reporting is shared by typed calls; it never queues generic core messages.
    pub(in crate::window) fn sent(&self, result: Result<(), puzzle_embed::ApiError>) {
        if let Err(error) = result {
            tracing::warn!(event = "desktop.surface_call_failed", %error);
        }
    }
}
