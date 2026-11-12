//! One permanent native link/context pair per display encountered by the process.
//!
//! Only control operations take this lock. The callback never reads the registry:
//! its stable context has clock atomics and an immutable wake-fanout snapshot.
//! In particular no lock needed by the callback is held across CoreVideo calls.
//! Entries persist through idle, unplugging and window close; native retained
//! addresses are never released, and contexts are never freed (user amendment).
use super::{ActorId, Entry, LinkError, Refresh, Wake};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

/// User-approved process-lifetime registry, growing only with distinct display IDs.
static ENTRIES: OnceLock<Mutex<BTreeMap<u32, Entry>>> = OnceLock::new();

/// A poisoned control lock can retain its complete entries without freeing anything.
fn entries() -> MutexGuard<'static, BTreeMap<u32, Entry>> {
    // Ownership changes are cold and serialized; the native callback has no
    // reference to this guard, so it can never wait behind a start/stop call.
    ENTRIES
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Each render actor supplies its own aggregate visible demand for a display.
pub(in crate::display_link) fn subscribe(
    display: u32,
    actor: ActorId,
    count: usize,
    wake: Wake,
) -> Result<(), LinkError> {
    let mut entries = entries();
    // Merely detaching an unseen display must not create a native source.
    if count == 0 && !entries.contains_key(&display) {
        return Ok(());
    }
    if let std::collections::btree_map::Entry::Vacant(entry) = entries.entry(display) {
        // Retaining the entry also retains registration errors: retries cannot
        // create another native object/context for this same display identity.
        entry.insert(Entry::new(display)?);
    }
    if let Some(entry) = entries.get_mut(&display) {
        entry.subscribe(actor, count, wake)?;
    }
    Ok(())
}

/// Read through this actor's cursor without consuming another App's tick.
pub(in crate::display_link) fn take_refresh(display: u32, actor: ActorId) -> Option<Refresh> {
    entries().get_mut(&display)?.take_refresh(actor)
}

/// Detach this actor everywhere, even if one unplugged display rejects stopping.
pub(in crate::display_link) fn stop_all(actor: ActorId, wake: &Wake) -> Result<(), LinkError> {
    let mut error = None;
    for entry in entries().values_mut() {
        if let Err(failure) = entry.subscribe(actor, 0, Arc::clone(wake)) {
            error.get_or_insert(failure);
        }
    }
    error.map_or(Ok(()), Err)
}

/// Installs one actual native source for the lifecycle test's discovered display.
#[cfg(test)]
pub(in crate::display_link) fn install_test_source() -> Result<u32, LinkError> {
    let (display, entry) = Entry::active_for_test()?;
    // Other tests use detached scalar mailboxes, so this is the only native
    // creation in the test process and remains reused throughout this witness.
    entries().insert(display, entry);
    Ok(display)
}
