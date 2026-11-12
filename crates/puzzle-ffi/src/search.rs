//! Per-surface native find UI ownership and asynchronous selection reads (D-61).
//!
//! Search execution and navigation already use the shared Session model. These
//! exports add only the missing UI choice and value query; neither reads a
//! Session on the host thread. Call success means queued, never completed.
//! Keeping this option as a setter preserves the ABI-v1 creation structure.
//! Hosts set it immediately after creation and before forwarding input.
//! Invalid choices are rejected before any owner message is admitted.
//! Selected text is returned by tick with the caller's correlation token, so
//! a component can discard an answer after its query UI has closed.
use crate::{abi::ResultCode, registry};
use puzzle_embed::SearchUi;

/// Select 0 builtIn (default) or 1 host; later input sees the option in FIFO order.
/// Host mode closes built-in query capture but preserves existing matches.
#[allow(
    unsafe_code,
    reason = "D-43: exported C option delegates to safe Surface API"
)]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_set_search_ui(id: u64, ui: u32) -> ResultCode {
    let ui = match ui {
        0 => SearchUi::BuiltIn,
        1 => SearchUi::Host,
        _ => return ResultCode::InvalidArgument,
    };
    registry::with_surface(id, |surface| surface.set_search_ui(ui))
}

/// Queues a plain selection read; selection/text returns {request,text} on app_tick.
/// No clipboard change or application input occurs; empty selection returns "".
#[allow(
    unsafe_code,
    reason = "D-43: exported C query delegates to safe Surface API"
)]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_selected_text(id: u64, request: u64) -> ResultCode {
    registry::with_surface(id, |surface| surface.selected_text(request))
}
