//! One test-only GPU lock for desktop device creation, loss and readbacks.
//!
//! The Rust test harness still runs CPU-only tests concurrently. GPU fixtures
//! hold this guard until all their devices and workers have been dropped, so
//! driver teardown cannot contend with another desktop fixture's readback.
use std::sync::{Mutex, MutexGuard};

/// Shared by every desktop GPU fixture, including explicitly ignored witnesses.
static GPU: Mutex<()> = Mutex::new(());

/// Acquires exclusive GPU-fixture ownership; an earlier failed test still fails
/// the harness, but poisoning must not prevent subsequent independent witnesses.
pub fn lock() -> MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
