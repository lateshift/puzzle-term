//! Native-host observation and notification seam for rendering (D-43).
//!
//! The host owns its window and supplies immutable display observations. The
//! renderer sends window operations back as notifications, so AppKit/WinUI/GTK
//! calls remain on their host thread. The wgpu surface is transferred separately:
//! its safe owner retains the native handle until the render driver drops it.
use super::surface::{Extent, SurfaceInfo};
#[cfg(feature = "lock-metrics")]
use crate::keyboard::raw::RawInput;
use std::sync::{Arc, Mutex, Weak};

/// Owned events for host operations, independent of render and Session ownership.
#[derive(Debug)]
pub enum HostAction {
    /// Coalesced pointer motion is flushed by the native input owner.
    FlushPointer,
    /// Cold-start fixture begins at the same handshake as Unicode release.
    #[cfg(feature = "lock-metrics")]
    StartReplay,
    /// Actual refresh callbacks advance a diagnostic position-only drag.
    #[cfg(feature = "lock-metrics")]
    ReplayTick {
        /// Native display whose callback drives this step.
        display: u64,
        /// Physical refresh identity deduplicates source observations.
        index: u64,
    },
    /// Explicit render damage posts a nonblocking frame request to the host.
    Frame,
    /// First successful drawable presentation, distinct from building or requesting one.
    FirstFrame,
    /// Presented caret cell in top-left logical pixels; None means outside the viewport.
    CursorRect(Option<[f32; 4]>),
    /// Session lifetime or a finite diagnostic requests host shutdown.
    Close,
    /// An opt-in input fixture uses the same UI-thread adapter as real OS input.
    #[cfg(feature = "lock-metrics")]
    Input {
        /// Recorded callback payload.
        input: RawInput,
        /// Stamp before posting to AppKit, so event-loop stalls remain measured.
        arrived: std::time::Instant,
    },
}

/// Display identity and visibility facts copied by the native host.
#[derive(Clone, Default)]
pub struct Visibility {
    /// Actual OS focus observation, distinct from terminal cursor focus.
    pub focused: bool,
    /// A host may have no visibility observation yet.
    pub visible: Option<bool>,
    /// Last actual occlusion notification; None remains unknown.
    pub occluded: Option<bool>,
    /// Diagnostic display resolution in device pixels, if reported.
    pub monitor_extent: Option<Extent>,
    /// Diagnostic monitor name, copied only at a host display transition.
    pub monitor_name: Option<String>,
    /// Fullscreen is a copied host fact for compositor diagnostics.
    #[cfg(feature = "lock-metrics")]
    pub fullscreen: bool,
    /// Actual monitor rate observation, rather than an invented phase sample.
    pub monitor_rate: Option<u32>,
}

/// Toolkit-free native layer observations beside the owned rendering surface.
#[derive(Clone)]
pub struct HostView {
    /// Geometry is refreshed only by coalesced host updates.
    pub info: SurfaceInfo,
    /// UI observations never query a toolkit from the render thread.
    pub visibility: Visibility,
    /// Notifications are posted to the host; this callback must never wait for it.
    notify: Arc<dyn Fn(HostAction) + Send + Sync>,
    /// Native contents metadata is queued before the GPU can publish readiness.
    #[cfg(target_os = "macos")]
    submit: Option<Arc<crate::app::presentation::Submissions>>,
    /// Survives context loans, geometry changes and device recreation for this surface.
    first_presented: bool,
    /// Outer None means no observation yet; inner None is the offscreen observation.
    cursor_rect: Option<Option<[f32; 4]>>,
    /// Read-only host mailbox gate; a crossing can arrive during a long frame build.
    updates: Weak<Mutex<super::surface::Pending>>,
}

/// Host notifications carry no mutable GPU or native-toolkit references.
impl HostView {
    /// Joins initial observations with the host's nonblocking notification endpoint.
    pub fn new(
        info: SurfaceInfo,
        visibility: Visibility,
        notify: Arc<dyn Fn(HostAction) + Send + Sync>,
    ) -> Self {
        Self {
            info,
            visibility,
            notify,
            #[cfg(target_os = "macos")]
            submit: None,
            first_presented: false,
            cursor_rect: None,
            updates: Weak::new(),
        }
    }
    /// Connects the actor mailbox without retaining its host or extending surface lifetime.
    pub(crate) fn attach_updates(&mut self, updates: Weak<Mutex<super::surface::Pending>>) {
        self.updates = updates;
    }
    /// A sealed old-display frame must not start another FIFO wait after a crossing.
    /// Size-only changes keep their D-54 clipped top-left frame; only the source
    /// display identity cancels acquisition. No Session or GPU data is locked.
    pub(crate) fn acquisition_current(&self) -> bool {
        let Some(updates) = self.updates.upgrade() else {
            // Standalone offscreen diagnostics have no asynchronous geometry owner.
            return true;
        };
        let updates = updates.lock().unwrap_or_else(|error| error.into_inner());
        updates
            .geometry
            .is_none_or(|info| info.display == self.info.display)
    }
    /// Posts diagnostic lifecycle events without a native window borrow.
    #[cfg(feature = "lock-metrics")]
    pub fn replay(&self, action: HostAction) {
        (self.notify)(action);
    }
    /// Returns the last forwarded backing-pixel extent without an OS call.
    pub fn inner_size(&self) -> Extent {
        self.info.extent
    }
    /// Returns the host's forwarded device scale without an OS call.
    pub fn scale_factor(&self) -> f64 {
        self.info.scale
    }
    /// Posts an explicit redraw request without constructing any frame on the host.
    pub fn request_redraw(&self) {
        (self.notify)(HostAction::Frame);
    }
    /// Requests native input to flush its most recent physical pointer sample.
    pub fn flush_pointer(&self) {
        (self.notify)(HostAction::FlushPointer);
    }
    /// Requests shutdown without waiting for a host callback or event-loop lock.
    pub fn close(&self) {
        (self.notify)(HostAction::Close);
    }
    /// Invalidates the host's caret observation after a coordinate-space change.
    /// Even equal logical rectangles must refresh native IME coordinates when
    /// backing scale, display or viewport dimensions have changed underneath them.
    pub(crate) fn geometry_changed(&mut self, info: SurfaceInfo) {
        if self.info != info {
            self.info = info;
            self.cursor_rect = None;
        }
    }
    /// Called only after submission and actual surface presentation succeeded.
    pub(crate) fn presented(&mut self, cursor: Option<[f32; 4]>) {
        if !self.first_presented {
            self.first_presented = true;
            (self.notify)(HostAction::FirstFrame);
        }
        // Repeated blink frames do not copy an unchanged caret event (P1/P11).
        if self.cursor_rect != Some(cursor) {
            self.cursor_rect = Some(cursor);
            (self.notify)(HostAction::CursorRect(cursor));
        }
    }
    /// Posts an explicitly activated diagnostic input through the real UI adapter.
    #[cfg(feature = "lock-metrics")]
    pub fn input(&self, input: RawInput) {
        (self.notify)(HostAction::Input {
            input,
            arrived: std::time::Instant::now(),
        });
    }
}

#[cfg(test)]
/// Presentation notifications are change-only and never originate from initialization.
mod tests;

/// Copied monitor metadata supplies diagnostic accessors without toolkit queries.
#[derive(Clone)]
pub struct Monitor {
    /// Reported native display dimensions.
    extent: Extent,
    /// Reported human-readable display identity.
    name: Option<String>,
    /// Actual reported refresh rate, if the host exposes one.
    rate: Option<u32>,
}
/// Diagnostic accessors read the last copied display facts.
impl Monitor {
    /// Device-pixel display dimensions copied at the host boundary.
    pub fn size(&self) -> Extent {
        self.extent
    }
    /// Display identity is allocated only by cold diagnostic observations.
    pub fn name(&self) -> Option<String> {
        self.name.clone()
    }
    /// The actual reported rate remains optional evidence.
    pub fn refresh_rate_millihertz(&self) -> Option<u32> {
        self.rate
    }
}
/// Native visibility remains an observation rather than a render-thread OS query.
impl HostView {
    /// Returns copied monitor metadata; never calls a toolkit from the render thread.
    pub fn current_monitor(&self) -> Option<Monitor> {
        self.visibility.monitor_extent.map(|extent| Monitor {
            extent,
            name: self.visibility.monitor_name.clone(),
            rate: self.visibility.monitor_rate,
        })
    }
    /// Returns the host's latest actual OS focus observation.
    pub fn has_focus(&self) -> bool {
        self.visibility.focused
    }
    /// Unknown native visibility stays unknown in calibration evidence.
    pub fn is_visible(&self) -> Option<bool> {
        self.visibility.visible
    }
    /// An optional marker preserves the diagnostic's windowed/fullscreen distinction.
    #[cfg(feature = "lock-metrics")]
    pub fn fullscreen(&self) -> Option<()> {
        self.visibility.fullscreen.then_some(())
    }
}

#[cfg(target_os = "macos")]
impl HostView {
    /// Cold wiring keeps native commit metadata out of the public host action enum.
    pub(crate) fn with_submission(
        mut self,
        submit: Arc<crate::app::presentation::Submissions>,
    ) -> Self {
        self.submit = Some(submit);
        self
    }
    /// Only queues metadata, never calls a native host or waits for its event loop.
    /// The CA commit consumes this table even while native resize tracking delays tick.
    pub(crate) fn submitted(&self, frame: crate::app::presentation::Submission) {
        if let Some(submit) = &self.submit {
            submit.submit(Some(frame));
        }
    }
}

#[cfg(target_os = "macos")]
impl HostView {
    /// A superseded completed image cannot retain its hit-table allocation.
    pub(crate) fn retire_submission(&self, serial: u64) {
        // This ordinal names a discarded native image, not a Session frame
        // request. Retiring it cannot acknowledge any later publication.
        if let Some(submit) = &self.submit {
            submit.retire(serial);
        }
    }
    /// Device loss/fence failure retires metadata that can never be presented.
    pub(crate) fn discard_submissions(&self) {
        if let Some(submit) = &self.submit {
            submit.submit(None);
        }
    }
}
