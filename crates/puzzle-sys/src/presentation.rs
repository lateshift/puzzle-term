//! D-60: IOSurface frames committed by a main-thread CALayer display callback.
//!
//! GPU completion only publishes a ready value and wakes the host. The main
//! thread alone changes layer contents. Ordinary frames never wait there;
//! geometry changes may wait for at most the display's current refresh period.
//! On timeout the previous image stays at its effective visual top left.
/// Geometry wait credit and geometric allocation growth.
mod geometry;
mod image;
/// Core Animation subclass and main-thread attachment lifetime.
mod layer;
/// Clock-injected matching and bounded receive policy, independent of native UI.
mod mailbox;
/// Bounded immutable GPU/compositor ownership and completion callbacks.
mod pool;
/// Main-only presented-image capability; snapshots never borrow a render mailbox.
mod snapshot;
pub use snapshot::{Snapshot, SnapshotError};
/// Native callback data is quiescent before synchronous host release returns.
mod wake;
pub use layer::Host;
use pool::Slot;
pub use pool::{RenderImage, Target};
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// A frame's physical extent and scale form one indivisible presentation key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    /// Physical BGRA pixels; zero-sized minimized layers do not request frames.
    pub size: [u32; 2],
    /// Device pixels per logical point, retained even at equal physical sizes.
    pub scale: f64,
}
/// A committed/retained layer observation for the host's acceptance trace.
#[derive(Clone, Copy, Debug)]
pub struct Receipt {
    /// Frame ordinal, independent of the physical display-link index.
    pub serial: Option<u64>,
    /// Actual geometry requested by Core Animation's display callback.
    pub requested: Geometry,
    /// Geometry of the IOSurface still displayed after the callback.
    pub committed: Option<Geometry>,
    /// Whole main-thread callback duration; present only on an activity receipt.
    pub main_busy: Option<Duration>,
    /// Wall time waiting for a new-size GPU completion, zero for ordinary frames.
    pub waited: Duration,
    /// A geometry-change wait exhausted its single-refresh bound.
    pub timed_out: bool,
}
/// Host-supplied geometry request only posts render messages, never calls Session.
pub type Request = Arc<dyn Fn(Geometry, Instant) + Send + Sync>;
/// A GPU completion may post only a wake; the host drains it on its own thread.
pub type Wake = Arc<dyn Fn() + Send + Sync>;
/// Receipts run on main after assigning contents, never on the Metal callback.
pub type Observe = Arc<dyn Fn(Receipt) + Send + Sync>;

/// Latest ready image coalesces without an output-dependent presentation queue.
struct Ready {
    /// Keeps backing storage unavailable for reuse until main adopts or discards it.
    slot: Arc<Slot>,
    /// The encoded frame that owns these pixels and its presented-region table.
    serial: u64,
    /// Exact rendered subrectangle, independent of allocation bucket dimensions.
    geometry: Geometry,
}
/// Close cancels every wait before any native view is released.
#[derive(Default)]
struct State {
    /// Encoded sRGB default clear, shared numerically with the UI owner.
    background: Option<[f32; 4]>,
    /// Latest host extent survives a declined display opportunity or reparenting.
    desired: Option<Geometry>,
    /// One pending geometry chain; GPU answers return its sole waiting credit.
    requests: geometry::Requests,
    /// No target can be written until main commits/releases a native transaction.
    pool_blocked: bool,
    /// Coalesced image ordinals retire their metadata on the render worker.
    /// Preallocated to pool capacity; drained before every further submission.
    retired: std::collections::VecDeque<u64>,
    /// Newest GPU-completed frame; old geometry never replaces a newer UI commit.
    ready: Option<Ready>,
    /// Main's last actually assigned image excludes reuse while CALayer references it.
    current: Option<Arc<Slot>>,
    /// A retained native layer callback becomes inert immediately at close.
    closed: bool,
}
/// Short locks protect immutable image ownership, never Session or GPU work.
struct Shared {
    /// Completion replaces a value under this guard, then wakes without holding it.
    state: Mutex<State>,
    /// Only geometry-change display callbacks may wait on this condition.
    ready: Condvar,
    /// GPU completion notifies the host's existing event loop.
    wake: wake::Gate,
    /// Host scale arrives before the CA transaction commits its changed bounds.
    scale: AtomicU64,
    /// One current display period bounds geometry-change waits without tuning.
    rate: AtomicU32,
}
impl Shared {
    /// Poisoning cannot make native teardown or a display callback unwind.
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
}

/// Sendable geometry facts contain no native object and never perform UI work.
#[derive(Clone)]
pub struct Control(Arc<Shared>);
impl Control {
    /// Forward the host's newest physical extent before its next CA transaction.
    pub fn resize(&self, geometry: Geometry, millihertz: u32) {
        self.geometry(geometry.scale, millihertz);
        // Reparenting may retain every front buffer from the previous window.
        // An obsolete ready image must not pin the only remaining writable slot.
        let changed = {
            let mut state = self.0.lock();
            let changed = state.desired != Some(geometry);
            state.desired = Some(geometry);
            state.retire_obsolete();
            changed
        };
        // Even if CA has already displayed old bounds, tick must apply the newest
        // host extent and retry asynchronous delivery. No native pointer crosses
        // this numeric control, and no foreign wake runs under the state guard.
        if changed {
            self.0.wake.notify();
        }
    }
    /// Called with the ordinary coalesced SurfaceInfo before CA lays out the layer.
    pub fn geometry(&self, scale: f64, millihertz: u32) {
        self.0.scale.store(scale.to_bits(), Ordering::Release);
        self.0.rate.store(millihertz, Ordering::Release);
    }
}

/// Geometry-change bounds, queue order and retained-image lifecycle regressions.
#[cfg(test)]
mod tests;
