//! Browser terminal: Session, WebGPU presentation, DOM input and typed host API (§12).
// A verbose release must use --no-default-features so tracing never sees
// both mutually exclusive static maximum levels in the same build.
#[cfg(all(feature = "release-info", feature = "verbose-logs"))]
compile_error!("verbose-logs requires --no-default-features");
// Browser tests exercise DOM input without requesting a GPU or initializing
// the page's tracing subscriber; the application entry point belongs to builds.
#[cfg(all(target_arch = "wasm32", feature = "review-scenes", not(test)))]
/// DOM canvas setup and asynchronous browser GPU initialization.
mod browser;
#[cfg(all(target_arch = "wasm32", not(feature = "review-scenes"), not(test)))]
/// Shipped diagnostic initialization without test-scene preparation.
mod browser_release;
/// Fixed-scene CSS and device-pixel dimensions shared with native tests.
#[cfg(any(feature = "review-scenes", test))]
mod canvas_size;
/// Owned DOM listeners and neutral input mapping for the browser host (§12.2).
#[cfg(target_arch = "wasm32")]
pub mod dom;

/// Browser transport credit and parsed-output provenance.
#[cfg(any(target_arch = "wasm32", test))]
pub mod transport;

#[cfg(target_arch = "wasm32")]
/// Session ownership, foreign callbacks and page-scoped resources.
mod runtime;

#[cfg(target_arch = "wasm32")]
/// The typed M7 embedding surface.
pub use runtime::api::{Connection, Disposable, PuzzleTerminal};

/// Optimized browser probes are compiled only for tests or the explicit dev page.
#[cfg(all(target_arch = "wasm32", any(test, feature = "measurements")))]
pub mod benchmarks;
