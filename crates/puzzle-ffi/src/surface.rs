//! Thin checked C surface operations; every mutation is a safe owner message.
//!
//! These exports do not inspect terminal state or wait for rendering. Host
//! transports retain unaccepted output suffixes; EOF follows accepted bytes.
//! Search counts and all other results arrive on the next App::tick delivery.
//!
//! A successful message call means accepted by the owner channel, not applied
//! synchronously. This is why search exposes its count in a later callback and
//! why visibility changes do not enter the render driver's mutable state here.
//! Runtime affinity and generation checks remain active in optimized builds.
//! The boolean and numeric validators reject values outside the C catalogue
//! before constructing Rust enums; no arbitrary integer is ever transmuted.
//!
//! Host byte transport uses different semantics from message strings: it borrows
//! the supplied bytes only while the safe API copies a prefix into its fixed pool.
//! An accepted count of zero is normal backpressure. The caller retains the suffix
//! and retries after a wake; this wrapper never allocates an unbounded byte queue.
//! The accepted output pointer is checked before admission so an invalid pointer
//! cannot consume bytes without reporting how many were accepted.
//! Exit is a separate message ordered after all accepted output; the safe transport
//! drains its pool before applying core child-exit policy.
//!
//! Settings patches clone this surface's resolved native store on the host thread.
//! Validation and clamping stay in the shared schema. The worker receives a copied
//! settings snapshot, never a borrowed JSON tree or another surface's state.
//! Invalid fields report an error while independent valid fields follow the schema's
//! existing partial-application behavior. Defaults are not replaced by C constants.
//! Program status has two opt-ins: its Cargo feature and this runtime registration.
//! Without the feature a valid surface reports Unsupported; an invalid handle still
//! reports its capability error before any optional protocol branch is considered.
//! Search navigation and ghost acceptance remain core actions, so overlays and
//! terminal modes have exactly the same behavior as they do in the Rust harness.
use crate::{
    abi::{Bytes, Geometry, ResultCode},
    input::boolean,
    memory, registry,
};
use puzzle_term::{messages::ExitInfo, search::SearchCommand, vt::search::FindOptions};

/// Forwards backing size, DPR and display identity together; latest observation wins.
///
/// Zero backing extent suspends presentation while preserving the native target.
/// The owner coalesces successive geometry observations before reflow.
#[allow(unsafe_code, reason = "D-43: C resize export carries copied geometry")]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_resize(id: u64, geometry: Geometry) -> ResultCode {
    let info = match crate::native::geometry(geometry) {
        Ok(info) => info,
        Err(error) => return error,
    };
    registry::with_surface(id, |surface| surface.resize(info))
}
/// Delivers focus before subsequent keys; terminal focus reports remain core policy.
///
/// Focus reports and hollow-cursor state follow the same ordered control inbox.
/// A later key cannot overtake this focus observation from the same host.
#[allow(unsafe_code, reason = "D-43: C focus message export")]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_focus(id: u64, focused: u32) -> ResultCode {
    let focused = match boolean(focused) {
        Ok(value) => value,
        Err(error) => return error,
    };
    registry::with_surface(id, |surface| surface.focus(focused))
}
/// Hiding/occluding stops subscriptions; showing requests the normal complete redraw.
///
/// Visibility and occlusion are independent observations from the host toolkit.
/// The render actor owns display subscription changes, including idle stopping.
#[allow(unsafe_code, reason = "D-43: copied host visibility export")]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_visibility(id: u64, visible: u32, occluded: u32) -> ResultCode {
    registry::result((|| {
        let visible = boolean(visible)?;
        let occluded = boolean(occluded)?;
        let mut registry = registry::lock()?;
        registry
            .surface(id)?
            .surface
            .visibility(puzzle_embed::Visibility {
                visible: Some(visible),
                occluded: Some(occluded),
                ..Default::default()
            })
            .map_err(ResultCode::from)
    })())
}
/// Starts/replaces literal search; case mode 0 smart, 1 sensitive, 2 insensitive.
#[doc = "# Safety"]
/// Query is a call-scoped readable UTF-8 Bytes range.
///
/// Smart case is represented by None, preserving the shared search engine policy.
/// A later search/state callback reports incremental progress and match count.
#[allow(unsafe_code, reason = "D-43: C search query marshalling")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_search(
    id: u64,
    query: Bytes,
    case_mode: u32,
) -> ResultCode {
    let sensitive = match case_mode {
        0 => None,
        1 => Some(true),
        2 => Some(false),
        _ => return ResultCode::InvalidArgument,
    };
    // SAFETY: host retains readable query memory through this synchronous copy.
    let query = match unsafe { memory::text(query) } {
        Ok(query) => query,
        Err(error) => return error,
    };
    registry::with_surface(id, |surface| {
        surface.search(SearchCommand::Find {
            query,
            options: FindOptions {
                case_sensitive: sensitive,
            },
        })
    })
}
/// Applies this surface's session-layer JSON patch through the common settings schema.
#[doc = "# Safety"]
/// Patch must remain readable for its declared length during this call.
///
/// The patch starts from this surface's resolved store, preserving independent overrides.
/// Valid fields apply even if another field is rejected by the schema.
#[allow(unsafe_code, reason = "R8/D-43: per-surface settings buffer")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_set_config(id: u64, patch: Bytes) -> ResultCode {
    registry::result((|| {
        let mut registry = registry::lock()?;
        let slot = registry.surface(id)?;
        // SAFETY: caller's patch memory is copied before any asynchronous owner work.
        let patch = unsafe { memory::text(patch)? };
        let mut settings = slot.settings.clone();
        let report = settings
            .apply_json(&patch)
            .map_err(|_| ResultCode::InvalidArgument)?;
        slot.surface
            .settings(settings.clone())
            .map_err(ResultCode::from)?;
        slot.settings = settings;
        if !report.rejected.is_empty() {
            return Err(ResultCode::InvalidArgument);
        }
        Ok(())
    })())
}
/// Admits a prefix into the fixed pool without blocking; retain and retry the suffix.
#[doc = "# Safety"]
/// Output is readable and accepted is writable for one size_t during this call.
///
/// Ok with zero accepted bytes is backpressure, not EOF and not a dropped buffer.
/// Only the accepted prefix may be released by the caller after this call.
#[allow(
    unsafe_code,
    reason = "D-43: bounded C host-transport input/output pointers"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_feed_output(
    id: u64,
    output: Bytes,
    accepted: *mut usize,
) -> ResultCode {
    registry::result((|| {
        memory::pointer(accepted)?;
        let mut registry = registry::lock()?;
        let surface = &registry.surface(id)?.surface;
        // SAFETY: host output bytes are borrowed only while the safe API copies its accepted prefix.
        let output = unsafe { memory::bytes(output)? };
        let count = surface.feed_output(output).map_err(ResultCode::from)?;
        // SAFETY: caller supplies writable size_t storage, validated before admitting any data.
        unsafe { memory::write(accepted, count) }
    })())
}
/// Reports drained host EOF; code_present 0 omits code, signal may be empty.
#[doc = "# Safety"]
/// Signal is a call-scoped readable UTF-8 buffer.
///
/// Exit is applied after the admitted byte pool drains, preserving the last screen.
/// An empty signal and absent code represent an unspecified host exit status.
#[allow(unsafe_code, reason = "D-43: host transport exit-status marshalling")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_exited(
    id: u64,
    code_present: u32,
    code: i32,
    signal: Bytes,
    runtime_ms: u64,
) -> ResultCode {
    let code = match boolean(code_present) {
        Ok(present) => present.then_some(code),
        Err(error) => return error,
    };
    // SAFETY: signal text is copied into the owned status before EOF is sent.
    let signal = match unsafe { memory::text(signal) } {
        Ok(signal) => signal,
        Err(error) => return error,
    };
    registry::with_surface(id, |surface| {
        surface.exited(ExitInfo {
            code,
            signal: (!signal.is_empty()).then_some(signal),
            runtime_ms,
        })
    })
}
/// Navigates to the next current literal-search match.
///
/// Navigation reuses the owner's match set and centers the selected result.
/// The host receives its new current-match index through search/state.
#[allow(unsafe_code, reason = "D-43: C search_next message export")]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_search_next(id: u64) -> ResultCode {
    registry::with_surface(id, |surface| surface.search_next())
}

/// Navigates to the previous match without scanning history on the host.
///
/// Reverse navigation keeps smart-case and tracked match positions unchanged.
/// No history scan occurs on the calling host thread.
#[allow(unsafe_code, reason = "D-43: C search_prev message export")]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_search_prev(id: u64) -> ResultCode {
    registry::with_surface(id, |surface| surface.search_prev())
}

/// Releases query results on the Session owner.
///
/// Clearing releases the query and its tracked results on the Session owner.
/// The next publication removes cell fills and scrollbar match markers together.
#[allow(unsafe_code, reason = "D-43: C search_clear message export")]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_search_clear(id: u64) -> ResultCode {
    registry::with_surface(id, |surface| surface.search_clear())
}

/// Accepts current suggestion through normal committed-input policy.
///
/// Acceptance uses the current Session suggestion, rather than stale host text.
/// The core clears that suggestion after committing it through input policy.
#[allow(unsafe_code, reason = "D-43: C ghost_accept message export")]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_ghost_accept(id: u64) -> ResultCode {
    registry::with_surface(id, |surface| surface.accept_ghost_text())
}

/// Requests an exposed-surface redraw without rendering on the host.
///
/// Expose notifications enqueue drawing work without encoding any input bytes.
/// The existing pacer decides the next eligible refresh and snapshot request.
#[allow(unsafe_code, reason = "D-43: C request_frame message export")]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_request_frame(id: u64) -> ResultCode {
    registry::with_surface(id, |surface| surface.request_frame())
}

/// Toggles the built-in performance overlay through its Session model.
///
/// The HUD is a built-in model; this call never reads render counters synchronously.
/// Its normal dirty/idle rules govern subsequent statistics publications.
#[allow(unsafe_code, reason = "D-43: debug overlay visibility export")]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_performance_hud(id: u64, shown: u32) -> ResultCode {
    let shown = match boolean(shown) {
        Ok(shown) => shown,
        Err(error) => return error,
    };
    registry::with_surface(id, |surface| surface.set_performance_hud(shown))
}

/// Returns credit for one whole transport/reply payload after actual writer consumption.
/// The receipt is copied, and owner-side validation rejects duplicates/out-of-order IDs.
#[allow(
    unsafe_code,
    reason = "D-43: C completion export contains only copied IDs"
)]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_reply_complete(id: u64, reply_id: u64) -> ResultCode {
    registry::with_surface(id, |surface| surface.reply_complete(reply_id))
}
