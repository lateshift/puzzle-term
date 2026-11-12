//! Actor-owned subscriptions to process-retained display links (D-53).
//!
//! Each App/render actor owns one facade. Native link/context pairs are shared
//! per display and retained until process exit; a facade removes only its own
//! demand at idle or teardown. The callback fans out immutable wake snapshots,
//! while each actor consumes clock facts through its own cursor. No App can steal
//! another's refresh or stop a link while another App still needs that display.
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// One output deadline supplied by the native refresh source.
#[derive(Clone, Copy, Debug)]
pub struct Refresh {
    /// First receipt after activation preserves start-to-first-callback evidence.
    /// This observation never participates in pacing or subscription decisions.
    pub restart: Option<(Instant, Instant)>,
    /// Native output-host timestamp: a physical refresh identity across restarts.
    pub index: u64,
    /// When CoreVideo expects this frame to reach the display.
    pub output: Instant,
    /// The display's actual period, without timer or acquisition calibration.
    pub period: Duration,
    /// Callback arrival permits wake-lag attribution independently of scanout.
    pub callback: Instant,
}

/// Platform errors leave the render actor alive and are reported by its host.
#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    /// Windows/Linux use their unchanged predicted-refresh timer path.
    #[error("display links are unsupported on this target")]
    Unsupported,
    /// CoreVideo rejected creation, callback registration, start or stop.
    #[error("CoreVideo returned {0}")]
    Native(i32),
    /// A successful native creation unexpectedly supplied no object.
    #[error("CoreVideo returned no display link")]
    Missing,
}

/// Callback work belongs to one render actor, never to a window.
type Wake = Arc<dyn Fn() + Send + Sync>;

/// Stable identity of one facade's uniquely allocated wake capability.
/// This address is compared only: it is never dereferenced or exposed to a host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ActorId {
    /// Arc's allocation cannot be reused while this facade is registered.
    address: usize,
}

/// One logical display-subscription registry belongs to each render actor.
///
/// On macOS this value stays on its creator thread, like the native link itself.
/// Hosts update subscriber counts after processing surface events and rendering;
/// zero detaches this actor; zero aggregate demand stops the shared source.
pub struct DisplayLinks {
    /// Native subscription membership is removed before this capability drops.
    actor: ActorId,
    /// A shared native source may fan out to several such independent inboxes.
    wake: Wake,
}

/// Platform operations are centralized here; the core never sees an OS callback.
impl DisplayLinks {
    /// Creates an actor facade; `wake` must only signal that actor's inbox.
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let wake: Wake = Arc::new(wake);
        // Arc always allocates its refcounts, even for a zero-sized closure.
        // Unregister precedes drop, so a recycled address has no old membership.
        let actor = ActorId {
            address: Arc::as_ptr(&wake) as *const () as usize,
        };
        Self { actor, wake }
    }

    /// Sets the number of visible surfaces currently needing frames on a display.
    pub fn subscribe(&mut self, display: u32, count: usize) -> Result<(), LinkError> {
        #[cfg(target_os = "macos")]
        {
            macos::registry::subscribe(display, self.actor, count, self.wake.clone())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (display, count, self.actor, &self.wake);
            Err(LinkError::Unsupported)
        }
    }

    /// Takes the latest coalesced tick once; no copying or allocation per frame.
    pub fn take_refresh(&mut self, display: u32) -> Option<Refresh> {
        #[cfg(target_os = "macos")]
        return macos::registry::take_refresh(display, self.actor);
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (display, self.actor);
            None
        }
    }

    /// Removes this actor from every source before its wake inbox is released.
    pub fn stop_all(&mut self) -> Result<(), LinkError> {
        #[cfg(target_os = "macos")]
        {
            macos::registry::stop_all(self.actor, &self.wake)
        }
        #[cfg(not(target_os = "macos"))]
        Err(LinkError::Unsupported)
    }
}

/// Native stop never frees memory which a straggler callback can still access.
impl Drop for DisplayLinks {
    fn drop(&mut self) {
        // Remove this actor even when another App keeps the same display alive.
        // Native/context ownership stays retained if the final native stop fails.
        let _ = self.stop_all();
    }
}

/// The sole foreign-call module; CVDisplayLink's deprecation is tracked in D-53.
#[cfg(target_os = "macos")]
#[allow(
    deprecated,
    reason = "D-53 supports macOS 13; NSView display links require 14"
)]
mod macos;
/// Atomic clock facts and immutable wake snapshots do not borrow actor state.
#[cfg(any(target_os = "macos", test))]
mod signal;
#[cfg(test)]
mod tests;
