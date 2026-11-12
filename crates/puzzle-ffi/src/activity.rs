//! Activity acknowledgement is a checked, nonblocking Session message.
//!
//! A hidden pane retains one activity edge until its host acknowledges it.
//! This wrapper owns no unread state and makes no guess from focus or time.
//! ABI v1 layouts stay unchanged; the new function is an additive export.

/// Acknowledge seen output; subsequent fresh output may emit surface/activity.
/// The safe Surface checks creating-thread affinity and closed state before send.
#[allow(
    unsafe_code,
    reason = "D-43: additive C activity acknowledgement export"
)]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_activity_seen(id: u64) -> crate::abi::ResultCode {
    // State belongs to Session, shared by every native host and the web core.
    crate::registry::with_surface(id, |surface| surface.activity_seen())
}
