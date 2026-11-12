//! Exported C calls reject malformed inputs and preserve generation/thread contracts.
#![allow(
    unsafe_code,
    clippy::unwrap_used,
    reason = "D-43: ABI tests own all foreign input allocations"
)]
use crate::{abi::*, *};
/// Pure snapshot pointer guards run without a native window (also under Miri).
mod snapshot;
#[cfg(not(miri))]
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// Only the creating test thread calls App operations; wake data is atomic and retained.
#[cfg(not(miri))]
extern "C" fn wake(userdata: *mut std::ffi::c_void) {
    // SAFETY: test retains the Arc allocation until app_free has joined every worker.
    let count = unsafe { &*userdata.cast::<AtomicUsize>() };
    count.fetch_add(1, Ordering::Relaxed);
}
/// Native initialization is excluded from Miri; handles and buffers run separately there.
#[cfg(not(miri))]
#[test]
fn exported_app_affinity_generations_and_null_inputs() {
    let count = Arc::new(AtomicUsize::new(0));
    let options = AppOptions {
        abi_version: 1,
        config_path: crate::memory::borrowed(b"/nonexistent-puzzle-ffi-test-config.json"),
        callbacks: Callbacks {
            userdata: Arc::as_ptr(&count).cast_mut().cast(),
            wakeup: Some(wake),
            event: None,
        },
    };
    let mut id = 0;

    assert_eq!(
        // SAFETY: null input is a specified rejection; writable output remains live.
        unsafe { puzzle_app_new(std::ptr::null(), &mut id) },
        ResultCode::InvalidArgument
    );
    // SAFETY: initialized options and output have their declared stack lifetimes.
    assert_eq!(unsafe { puzzle_app_new(&options, &mut id) }, ResultCode::Ok);
    let foreign = std::thread::spawn(move || {
        assert_eq!(puzzle_app_tick(id), ResultCode::WrongThread);
        assert_eq!(puzzle_app_free(id), ResultCode::WrongThread);
        optional_foreign_thread(id);
    });
    foreign.join().unwrap();
    assert_eq!(puzzle_app_tick(id), ResultCode::Ok);

    assert_eq!(
        // SAFETY: invalid UTF-8 literal is readable and must fail before configuration parsing.
        unsafe { puzzle_app_set_config(id, crate::memory::borrowed(&[0xff])) },
        ResultCode::InvalidArgument
    );

    assert_eq!(
        // SAFETY: null surface options are rejected before native attachment; output is writable.
        unsafe { puzzle_surface_new(id, std::ptr::null(), &mut 0) },
        ResultCode::InvalidArgument
    );
    assert_eq!(puzzle_app_free(id), ResultCode::Ok);
    assert_eq!(puzzle_app_tick(id), ResultCode::InvalidHandle);
    // Reusing a registry slot must issue a new generation.
    let mut replacement = 0;

    assert_eq!(
        // SAFETY: options and new output both remain allocated through this call.
        unsafe { puzzle_app_new(&options, &mut replacement) },
        ResultCode::Ok
    );
    assert_ne!(id, replacement);
    assert_eq!(puzzle_app_free(id), ResultCode::InvalidHandle);
    assert_eq!(puzzle_app_free(replacement), ResultCode::Ok);
}
/// Null/oversized input errors never dereference bogus buffers or reach a native object.
#[test]
fn exported_surface_misuse_never_addresses_native_state() {
    // Reply receipts are scalar values: null/stale surfaces never reach native I/O.
    assert_eq!(
        puzzle_surface_reply_complete(0, 1),
        ResultCode::InvalidHandle
    );
    assert_eq!(
        puzzle_surface_reply_complete(0, 0),
        ResultCode::InvalidHandle
    );
    let empty = Bytes {
        data: std::ptr::null(),
        len: 0,
    };
    let huge = Bytes {
        data: b"x".as_ptr(),
        len: usize::MAX,
    };

    assert_eq!(
        // SAFETY: oversized input is rejected before reading outside the one-byte literal.
        unsafe { puzzle_surface_text(0, huge) },
        ResultCode::InvalidArgument
    );

    assert_eq!(
        // SAFETY: empty null input buffers are valid and only the stale ID is rejected.
        unsafe { puzzle_surface_paste(0, empty) },
        ResultCode::InvalidHandle
    );

    assert_eq!(
        // SAFETY: clipboard answer borrows no bytes from this empty buffer.
        unsafe { puzzle_surface_clipboard_answer(0, empty) },
        ResultCode::InvalidHandle
    );

    assert_eq!(
        // SAFETY: the empty suggestion is a valid clear operation on a rejected ID.
        unsafe { puzzle_surface_ghost_text(0, empty) },
        ResultCode::InvalidHandle
    );

    assert_eq!(
        // SAFETY: empty preedit with absent caret needs no input dereference.
        unsafe { puzzle_surface_preedit(0, empty, usize::MAX) },
        ResultCode::InvalidHandle
    );
}
/// Capability-only exports reject zero without any foreign memory access.
#[test]
fn exported_capabilities_reject_zero() {
    assert_eq!(puzzle_surface_free(0), ResultCode::InvalidHandle);
    assert_eq!(puzzle_surface_focus(0, 1), ResultCode::InvalidHandle);
    assert_eq!(puzzle_surface_focus(0, 2), ResultCode::InvalidArgument);
    assert_eq!(
        puzzle_surface_visibility(0, 1, 0),
        ResultCode::InvalidHandle
    );
    assert_eq!(puzzle_surface_search_next(0), ResultCode::InvalidHandle);
    assert_eq!(puzzle_surface_search_prev(0), ResultCode::InvalidHandle);
    assert_eq!(puzzle_surface_search_clear(0), ResultCode::InvalidHandle);
    assert_eq!(puzzle_surface_ghost_accept(0), ResultCode::InvalidHandle);
    assert_eq!(puzzle_surface_request_frame(0), ResultCode::InvalidHandle);
    assert_eq!(
        puzzle_surface_performance_hud(0, 1),
        ResultCode::InvalidHandle
    );
    optional_invalid_handles();
    assert_eq!(
        puzzle_surface_marker_add(0, 1, 0, 0),
        ResultCode::InvalidHandle
    );
    assert_eq!(
        puzzle_surface_marker_remove(0, 0),
        ResultCode::InvalidHandle
    );
    assert_eq!(
        puzzle_surface_decoration_remove(0, 0),
        ResultCode::InvalidHandle
    );
}

/// Invalid structured inputs and geometry cover the remaining exported messages.
mod arguments;

/// Malformed MIME buffers fail before native creation, so Miri covers the ABI guard.
#[test]
fn clipboard_mime_invalid_ranges_and_statuses() {
    use abi::{Bytes, MimeItem, ResultCode};
    let empty = Bytes {
        data: std::ptr::null(),
        len: 0,
    };
    assert_eq!(
        // SAFETY: empty array needs no foreign allocation; bad status is rejected first.
        unsafe { puzzle_surface_clipboard_mime_answer(0, 1, 99, u64::MAX, std::ptr::null(), 0) },
        ResultCode::InvalidArgument
    );
    assert_eq!(
        // SAFETY: null nonempty array is rejected before dereference.
        unsafe { puzzle_surface_clipboard_mime_answer(0, 1, 0, u64::MAX, std::ptr::null(), 1) },
        ResultCode::InvalidArgument
    );
    let item = MimeItem {
        mime: empty,
        data: empty,
    };
    assert_eq!(
        // SAFETY: item and its empty ranges remain live for the synchronous checked copy.
        unsafe { puzzle_surface_clipboard_mime_answer(0, 1, 0, u64::MAX, &item, 1) },
        ResultCode::InvalidArgument
    );
    let item = MimeItem {
        mime: memory::borrowed(b"image/png"),
        data: memory::borrowed(&[0, 255, 1]),
    };
    assert_eq!(
        // SAFETY: stack item and its static buffers are valid until this call returns.
        unsafe { puzzle_surface_clipboard_mime_answer(0, 1, 0, u64::MAX, &item, 1) },
        ResultCode::InvalidHandle
    );
}

/// Gesture arrays and native revisions are checked before any OS random or clipboard access.
#[test]
fn clipboard_paste_rejects_invalid_foreign_arguments() {
    let empty = abi::Bytes {
        data: std::ptr::null(),
        len: 0,
    };
    let mime = memory::borrowed(b"text/plain");
    for (primary, revision, pointer, count) in [
        (2, 1, &mime as *const _, 1),
        (0, u64::MAX, &mime as *const _, 1),
        (0, 1, std::ptr::null(), 1),
        (0, 1, std::ptr::null(), 65),
    ] {
        // SAFETY: each live local is call-scoped; malformed null/count pairs are rejected first.
        let result =
            unsafe { puzzle_surface_clipboard_paste(0, primary, revision, pointer, count, empty) };
        assert_eq!(result, abi::ResultCode::InvalidArgument);
    }
    // SAFETY: the one initialized MIME and empty fallback remain live for the checked copy.
    let result = unsafe { puzzle_surface_clipboard_paste(0, 0, 1, &mime, 1, empty) };
    assert_eq!(result, abi::ResultCode::InvalidHandle);
}

/// Notification feedback rejects malformed call-scoped bytes before native lookup.
#[test]
fn notification_feedback_rejects_invalid_bytes_and_tags() {
    for (bytes, tag, expected) in [
        (crate::memory::borrowed(b""), 0, ResultCode::InvalidArgument),
        (
            crate::memory::borrowed(&[0xff]),
            0,
            ResultCode::InvalidArgument,
        ),
        (
            crate::memory::borrowed(b"1:1"),
            4,
            ResultCode::InvalidArgument,
        ),
        (
            crate::memory::borrowed(b"1:1"),
            0,
            ResultCode::InvalidHandle,
        ),
        (
            Bytes {
                data: std::ptr::null(),
                len: 1,
            },
            0,
            ResultCode::InvalidArgument,
        ),
    ] {
        assert_eq!(
            // SAFETY: literals remain live, and the invalid null range must be rejected.
            unsafe { puzzle_surface_notification_feedback(0, bytes, tag) },
            expected
        );
    }
}

/// Native drop argument guards run without creating a GPU or opening user files.
#[test]
fn drop_rejects_invalid_ranges_and_tags() {
    let item = MimeItem {
        mime: memory::borrowed(b"text/plain"),
        data: memory::borrowed(b"x"),
    };
    for (generation, kind, x, ops, pointer, count) in [
        (0, 2, 0.0, 1, &item as *const _, 1),
        (1, 3, 0.0, 1, &item as *const _, 1),
        (1, 2, f64::NAN, 1, &item as *const _, 1),
        (1, 2, 0.0, 4, &item as *const _, 1),
        (1, 1, 0.0, 1, &item as *const _, 1),
        (1, 0, 0.0, 1, &item as *const _, 1),
        (1, 2, 0.0, 1, std::ptr::null(), 1),
        (1, 2, 0.0, 1, std::ptr::null(), 17),
    ] {
        assert_eq!(
            // SAFETY: initialized item is live; invalid null/count pairs must reject before reading.
            unsafe { puzzle_surface_drop(0, generation, kind, x, 0.0, ops, pointer, count) },
            ResultCode::InvalidArgument
        );
    }
    assert_eq!(
        // SAFETY: valid data is live for this call; only the nonexistent surface is rejected.
        unsafe { puzzle_surface_drop(0, 1, 2, 0.0, 0.0, 1, &item, 1) },
        ResultCode::InvalidHandle
    );
}

// Extra protocol assertions are omitted with their implementation.
include!("tests/optional_items.rs");
