//! Host-neutral surface facts passed to the native rendering actor (D-43).
//!
//! A host creates its safe wgpu surface on the required platform thread and
//! transfers it with these observations. No OS window or toolkit object is
//! needed by the frame scheduler. Geometry changes overwrite one another;
//! damage and timer requests survive independently of the latest geometry.
use std::time::{Duration, Instant};

/// Host-provided physical refresh facts; embedding hosts can supply this same value.
#[derive(Clone, Copy, Debug)]
pub struct Refresh {
    /// Display identity prevents a queued old-display callback from retiming a move.
    pub display: u64,
    /// Physical output timestamp identifies this refresh across source restarts.
    pub index: u64,
    /// Native source's next output time, independent of OS timer delivery.
    pub output: Instant,
    /// Measured native refresh period, rather than an acquisition phase guess.
    pub period: Duration,
    /// Callback time distinguishes source delivery from actor wake delay.
    pub callback: Instant,
}

/// Stable identity in the process render worker, independent of winit WindowId.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SurfaceId(pub u64);

/// Device-pixel extent with no toolkit-specific coordinate type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    /// Actual backing pixels across the native layer.
    pub width: u32,
    /// Actual backing pixels down the native layer.
    pub height: u32,
}

/// The current native layer's geometry and display clock observations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceInfo {
    /// Zero extent denotes a minimized layer; configuring it is deferred.
    pub extent: Extent,
    /// Device pixels per logical pixel, used by metrics and hinting.
    pub scale: f64,
    /// macOS CGDirectDisplayID, or another host's display incarnation.
    pub display: u64,
    /// Highest advertised monitor rate, or the documented 60-Hz fallback.
    pub millihertz: u32,
}

/// First damage and its preceding output preserve D-33 across coalesced wakes.
#[derive(Clone, Copy, Debug)]
pub struct DamageArrival {
    /// Actual first request after a snapshot; delivery may happen much later.
    pub first: Instant,
    /// Last request, including suppressed wakes, before this first request.
    pub previous: Option<Instant>,
}

/// One host update; high-frequency variants merge rather than queueing frames.
pub enum SurfaceUpdate {
    /// Selects host pacing for this surface's remaining lifetime and supplies a tick.
    HostRefresh(Refresh),
    /// A host's display link may drive the same seam as the harness source (M17).
    Refresh(Refresh),
    /// VT damage or input requests the next paced snapshot.
    Frame,
    /// Change-only Session animation state controls settling without a query.
    Animation(bool),
    /// Titles are observational metadata for explicit native diagnostics.
    Title(String),
    /// The prior presented region allocation returns for render-side reuse.
    Regions(puzzle_term::overlay::regions::PresentedRegions),
    /// Coalesced parser damage carries actual burst timing independently of delivery.
    Output(DamageArrival),
    /// A completion supplies prepared ink without changing the output-burst boundary.
    Fallback,
    /// Latest native layer size, scale and refresh observations win.
    Geometry(SurfaceInfo),
    /// D-60: CA needs these exact bounds before its bounded transaction deadline.
    #[cfg(target_os = "macos")]
    GeometryFrame(SurfaceInfo, Instant),
    /// Last-good host settings include immediate input policy and deferred font work.
    Settings(Box<crate::settings::DesktopSettings>),
    /// Focus/visibility observations are copied on the native thread.
    Visibility(super::host::Visibility),
    /// Blink, sync timeout or an overlay explicitly requests this one deadline.
    Timer(Instant),
}

/// Reusable merged update state taken at a render-thread boundary.
#[derive(Default)]
pub struct Pending {
    /// Sticky actor observation: host pacing replaces the native source until close.
    pub host_driven: bool,
    /// Latest physical refresh coalesces while a preceding frame is being built.
    pub refresh: Option<Refresh>,
    /// Completed actor wait is inert evidence, excluded from mailbox emptiness.
    #[cfg(feature = "lock-metrics")]
    pub actor_wake: Option<super::wake::Sample>,
    /// Diagnostic native link start and its first callback; never used by pacing.
    #[cfg(feature = "lock-metrics")]
    pub display_restart: Option<(Instant, Instant)>,
    /// At least one VT or input redraw request arrived since the last take.
    pub frame: bool,
    /// Latest animation edge wins independently of damage.
    pub animation: Option<bool>,
    /// Rare metadata changes retain no reference into live terminal state.
    pub title: Option<String>,
    /// Region storage is reused on its drawing owner.
    pub regions: Option<puzzle_term::overlay::regions::PresentedRegions>,
    /// Earliest output arrival preserves the start of an isolated editor update.
    pub output: Option<DamageArrival>,
    /// At least one native glyph completion arrived since the last take.
    pub fallback: bool,
    /// Intermediate drag sizes are replaced without growing a queue.
    pub geometry: Option<SurfaceInfo>,
    /// First host arrival starts the throttle even when a render turn is delayed.
    pub geometry_at: Option<Instant>,
    /// Only a changed-geometry CALayer callback supplies a synchronous deadline.
    #[cfg(target_os = "macos")]
    pub geometry_deadline: Option<Instant>,
    /// Only the newest validated settings snapshot is needed by the renderer.
    pub settings: Option<Box<crate::settings::DesktopSettings>>,
    /// Latest native focus and occlusion observations.
    pub visibility: Option<super::host::Visibility>,
    /// The earliest explicit timer cannot be delayed by a later request.
    pub timer: Option<Instant>,
}

impl Pending {
    /// Combines independent requests while preserving latest-size and earliest-timer semantics.
    pub fn merge(&mut self, update: SurfaceUpdate) {
        match update {
            SurfaceUpdate::HostRefresh(refresh) => {
                self.host_driven = true;
                // Delivery can be coalesced or reordered by a host's main queue.
                // Keep the newest identity on one display, never regress it.
                if self
                    .refresh
                    .is_none_or(|old| old.display != refresh.display || old.index < refresh.index)
                {
                    self.refresh = Some(refresh);
                }
            }
            SurfaceUpdate::Refresh(refresh) => self.refresh = Some(refresh),
            SurfaceUpdate::Frame => self.frame = true,
            SurfaceUpdate::Animation(active) => self.animation = Some(active),
            SurfaceUpdate::Title(title) => self.title = Some(title),
            SurfaceUpdate::Regions(regions) => self.regions = Some(regions),
            SurfaceUpdate::Output(arrival) => {
                // Retain the first arrival if startup/expose captured before UI delivery.
                if self.output.is_none_or(|old| arrival.first < old.first) {
                    self.output = Some(arrival);
                }
            }
            SurfaceUpdate::Fallback => self.fallback = true,
            SurfaceUpdate::Geometry(info) => {
                self.geometry = Some(info);
                // Preserve the first arrival while replacing the extent: dequeue time
                // or a later event must not restart the PTY throttle window.
                self.geometry_at.get_or_insert_with(Instant::now);
            }
            #[cfg(target_os = "macos")]
            SurfaceUpdate::GeometryFrame(info, deadline) => {
                self.geometry = Some(info);
                self.geometry_at.get_or_insert_with(Instant::now);
                self.geometry_deadline = Some(deadline);
            }
            SurfaceUpdate::Settings(settings) => self.settings = Some(settings),
            SurfaceUpdate::Visibility(visibility) => self.visibility = Some(visibility),
            SurfaceUpdate::Timer(due) => {
                self.timer = Some(self.timer.map_or(due, |old| old.min(due)))
            }
        }
    }

    /// Allows an idle driver to skip work without peeking at a GPU or terminal.
    pub fn is_empty(&self) -> bool {
        self.refresh.is_none()
            && !self.host_driven
            && !self.frame
            && self.animation.is_none()
            && self.title.is_none()
            && self.regions.is_none()
            && self.output.is_none()
            && !self.fallback
            && self.geometry.is_none()
            && self.timer.is_none()
            && self.settings.is_none()
            && self.visibility.is_none()
    }
}
