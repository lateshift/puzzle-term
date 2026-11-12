//! Process C handle registry with checked host-thread ownership (M17.ffi).
//!
//! The registry exists because C IDs cannot carry Rust ownership. The mutex
//! protects only cold handle metadata and host calls, never a Session or a GPU
//! turn. No reference escapes its guard. Native App access and destruction are
//! permitted only after checking the creating thread, in every build profile.
//! Tick releases this guard before invoking callbacks; lifecycle changes during
//! delivery are Busy, preventing callback userdata from dying mid-delivery.
use crate::{
    abi::{Callbacks, ResultCode},
    handles::Table,
};
use puzzle_embed::{App, DesktopSettings, Surface};
use std::{
    collections::BTreeMap,
    sync::{Mutex, MutexGuard},
    thread::{self, ThreadId},
};

/// Host-affine App allocation; only identity metadata may be read on another thread.
pub(crate) struct AppSlot {
    /// Checked before any App access or ownership removal.
    owner: ThreadId,
    /// Drop happens exclusively in app_free on owner, outside the registry lock.
    pub(crate) app: App,
    /// Current default settings also seed per-surface configuration patches.
    pub(crate) settings: DesktopSettings,
    /// Opaque callback identity; the library never dereferences userdata.
    pub(crate) userdata: usize,
    /// Function pointers have no Rust borrow; C guarantees their callback lifetime.
    pub(crate) callbacks: Callbacks,
    /// Old surface IDs remain completion tokens until native release acknowledgement.
    pub(crate) identities: BTreeMap<puzzle_embed::SurfaceId, u64>,
    /// Prevents recursive tick and destruction while callback buffers are live.
    pub(crate) delivering: bool,
}
#[allow(
    unsafe_code,
    reason = "R8/D-43: C registry enforces native App affinity dynamically"
)]
// SAFETY: all App access/drop is preceded by owner-thread validation under REGISTRY;
// no reference escapes its guard, the static never drops entries, and callbacks run unlocked.
unsafe impl Send for AppSlot {}
impl AppSlot {
    /// Transfers one newly created App into the owner-checked C namespace.
    pub(crate) fn new(app: App, settings: DesktopSettings, callbacks: Callbacks) -> Self {
        Self {
            owner: thread::current().id(),
            app,
            settings,
            userdata: callbacks.userdata as usize,
            callbacks,
            identities: BTreeMap::new(),
            delivering: false,
        }
    }
    /// Foreign callers may inspect only the owner ID before returning an error.
    pub(crate) fn check(&self) -> Result<(), ResultCode> {
        if self.owner != thread::current().id() {
            return Err(ResultCode::WrongThread);
        }
        Ok(())
    }
}
/// Surface capabilities refer back to the App which owns cancellation and callbacks.
pub(crate) struct SurfaceSlot {
    /// Generation-checked owning App ID, never a raw allocation address.
    pub(crate) app: u64,
    /// Every method performs its additional native affinity/lifetime checks.
    pub(crate) surface: Surface,
    /// Settings updates retain this surface's independent resolved snapshot.
    pub(crate) settings: DesktopSettings,
    /// Marker IDs stay local to this surface and reject stale incarnations.
    pub(crate) markers: Table<puzzle_term::overlay::anchor::Marker, false>,
}
/// Only the platform boundary holds this process registry; core state stays threadless.
pub(crate) struct Registry {
    /// App namespace has the high ID bit clear.
    pub(crate) apps: Table<AppSlot, false>,
    /// Surface namespace has the high ID bit set.
    pub(crate) surfaces: Table<SurfaceSlot, true>,
}
/// C callers cannot own a Rust registry, so this static owns only checked capabilities.
static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    apps: Table::new(),
    surfaces: Table::new(),
});
/// Poison indicates an internal bug; reject further calls rather than unwinding across C.
pub(crate) fn lock() -> Result<MutexGuard<'static, Registry>, ResultCode> {
    REGISTRY.lock().map_err(|_| ResultCode::Platform)
}
impl Registry {
    /// Native app operations require affinity even when a handle is otherwise valid.
    pub(crate) fn app(&mut self, id: u64) -> Result<&mut AppSlot, ResultCode> {
        self.apps.get(id)?.check()?;
        self.apps.get_mut(id)
    }
    /// Surface owner validation precedes revealing its checked input capability.
    pub(crate) fn surface(&mut self, id: u64) -> Result<&mut SurfaceSlot, ResultCode> {
        let owner = self.surfaces.get(id)?.app;
        self.apps.get(owner)?.check()?;
        self.surfaces.get_mut(id)
    }
}
/// One small error mapping is shared by exports; no error unwinds into a C caller.
pub(crate) fn result(value: Result<(), ResultCode>) -> ResultCode {
    value.err().unwrap_or(ResultCode::Ok)
}
/// Safe surface methods are the only place where C calls can reach terminal work.
pub(crate) fn with_surface(
    id: u64,
    operation: impl FnOnce(&Surface) -> Result<(), puzzle_embed::ApiError>,
) -> ResultCode {
    result((|| {
        let mut registry = lock()?;
        operation(&registry.surface(id)?.surface).map_err(ResultCode::from)
    })())
}
