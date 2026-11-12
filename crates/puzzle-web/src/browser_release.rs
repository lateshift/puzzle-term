//! Shipped startup installs diagnostics; the JS host creates the full terminal.
//! GPU requests belong to PuzzleTerminal.create and the approved page registry,
//! rather than a second clear-only device created by the wasm start hook.
use std::str::FromStr;
use tracing_subscriber::{filter::Targets, prelude::*};
use wasm_bindgen::prelude::*;

/// Reads the release log filter while retaining the first-frame info event.
fn log_targets() -> Targets {
    // The web default is quiet enough for normal page use, while the
    // first-frame info record remains visible for the review checklist.
    let default = Targets::new()
        .with_default(tracing::Level::WARN)
        .with_target("puzzle_web", tracing::Level::INFO);
    let query = web_sys::window()
        // A missing or malformed query must not prevent first presentation.
        .and_then(|window| window.location().search().ok())
        .and_then(|search| web_sys::UrlSearchParams::new_with_str(&search).ok())
        .and_then(|params| params.get("log"));
    // Invalid filter syntax falls back to the known-safe default instead
    // of failing the browser start hook before WebGPU can report its state.
    query
        .and_then(|spec| Targets::from_str(&spec).ok())
        .unwrap_or(default)
}

/// Starts asynchronous WebGPU setup after tracing is ready for failures.
#[wasm_bindgen(start)]
pub(crate) fn start() {
    // Browser panic stacks are useful during development, but ordinary
    // lifecycle and validation failures flow through tracing below.
    console_error_panic_hook::set_once();
    // The console already supplies timestamps; performance spans use the
    // separate browser layer and can be inspected in the Performance panel.
    let console = tracing_subscriber::fmt::layer()
        .without_time()
        .with_writer(tracing_web::MakeWebConsoleWriter::new())
        .with_filter(log_targets());
    tracing_subscriber::registry()
        .with(console)
        .with(tracing_web::performance_layer())
        .init();
}
