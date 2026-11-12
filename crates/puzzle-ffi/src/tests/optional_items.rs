// Standard invalid-handle tests have no extra protocol exports.
/// Base calls are exercised by the surrounding fixture.
fn optional_invalid_handles() {}

/// The standard interface declares no additional foreign-thread operation.
fn optional_foreign_thread(_: u64) {}
