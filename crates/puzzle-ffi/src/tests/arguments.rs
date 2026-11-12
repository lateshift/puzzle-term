//! Exported marshalling rejects malformed shapes before any native target is addressed.
#![allow(
    unsafe_code,
    reason = "D-43: ABI tests pass explicit null or live stack inputs"
)]
use crate::{abi::*, *};

#[test]
fn structured_inputs_cannot_reach_a_stale_surface() {
    let null = std::ptr::null();
    // SAFETY: the stale capability is rejected without reading the null key.
    let key = unsafe { puzzle_surface_key(0, null) };
    assert_eq!(key, ResultCode::InvalidHandle);
    // SAFETY: a null pointer sample is rejected before constructing a reference.
    let pointer = unsafe { puzzle_surface_pointer(0, std::ptr::null()) };
    assert_eq!(pointer, ResultCode::InvalidArgument);
    // SAFETY: null location storage is a documented rejection, not a dereference.
    let scroll = unsafe { puzzle_surface_scroll(0, 1.0, 0, std::ptr::null()) };
    assert_eq!(scroll, ResultCode::InvalidArgument);
    // SAFETY: invalid unit must fail before reading the deliberately null pointer.
    let unit = unsafe { puzzle_surface_scroll(0, 1.0, 2, std::ptr::null()) };
    assert_eq!(unit, ResultCode::InvalidArgument);
    // SAFETY: declaration conversion is never entered for a stale surface ID.
    let add = unsafe { puzzle_surface_decoration_add(0, 1, std::ptr::null()) };
    assert_eq!(add, ResultCode::InvalidHandle);
    // SAFETY: replacement input is not accessed after the capability lookup fails.
    let update = unsafe { puzzle_surface_decoration_update(0, 1, std::ptr::null()) };
    assert_eq!(update, ResultCode::InvalidHandle);
}
#[test]
fn search_transport_and_geometry_reject_invalid_arguments() {
    let empty = Bytes {
        data: std::ptr::null(),
        len: 0,
    };
    // SAFETY: empty search borrows no memory and only the stale surface is rejected.
    let search = unsafe { puzzle_surface_search(0, empty, 0) };
    assert_eq!(search, ResultCode::InvalidHandle);
    // SAFETY: empty patch is never parsed for a rejected capability.
    let config = unsafe { puzzle_surface_set_config(0, empty) };
    assert_eq!(config, ResultCode::InvalidHandle);
    // SAFETY: null accepted destination is rejected before any pool admission.
    let feed = unsafe { puzzle_surface_feed_output(0, empty, std::ptr::null_mut()) };
    assert_eq!(feed, ResultCode::InvalidArgument);
    // SAFETY: empty signal borrows no allocation; host exit checks the ID next.
    let exit = unsafe { puzzle_surface_exited(0, 1, 0, empty, 0) };
    assert_eq!(exit, ResultCode::InvalidHandle);
    let geometry = Geometry {
        width: 1,
        height: 1,
        scale: f64::NAN,
        display: 0,
        millihertz: 60_000,
    };
    assert_eq!(
        puzzle_surface_resize(0, geometry),
        ResultCode::InvalidArgument
    );
}
