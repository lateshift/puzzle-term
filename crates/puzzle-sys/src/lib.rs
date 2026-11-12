//! Safe platform facilities with tightly scoped foreign-call glue (D-53/R8).
//!
//! macOS display links retain native objects and callback contexts for the process
//! lifetime. Surface subscriptions are detachable; no callback owns a window.
#![deny(
    clippy::undocumented_unsafe_blocks,
    clippy::multiple_unsafe_ops_per_block
)]

/// Display-paced wakeups; foreign objects never contain surface pointers.
pub mod display_link;

/// Unscaled top-left preservation of the last macOS drawable (D-54).
#[cfg(target_os = "macos")]
pub mod layer;

/// IOSurface-backed native presentation and bounded geometry-change commits (D-60).
#[cfg(target_os = "macos")]
pub mod presentation;

/// Bounds Objective-C temporary ownership to one native render turn.
/// Metal creates autoreleased internal objects even when returned handles are retained;
/// worker threads have no AppKit run-loop pool to release those temporaries for them.
/// Explicit retained resources may escape, but temporary command/texture references
/// are released before this worker sleeps. Other platforms need no native scope.
pub fn with_autorelease_pool<T>(operation: impl FnOnce() -> T) -> T {
    #[cfg(target_os = "macos")]
    {
        objc2::rc::autoreleasepool(|_| operation())
    }
    #[cfg(not(target_os = "macos"))]
    {
        operation()
    }
}
