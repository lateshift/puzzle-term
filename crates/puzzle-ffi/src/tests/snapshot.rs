//! Output pointer guards reject invalid memory before borrowing a host image.
use super::*;

#[test]
fn snapshot_rejects_invalid_buffers_and_stale_handles_without_writes() {
    let mut width = 9;
    let mut height = 9;
    let mut bytes = [0x5a; 4];
    for (buffer, capacity) in [(std::ptr::null_mut(), 4), (bytes.as_mut_ptr(), usize::MAX)] {
        assert_eq!(
            // SAFETY: initialized outputs are live; invalid byte extents must reject.
            unsafe { puzzle_surface_snapshot(0, 360, buffer, capacity, &mut width, &mut height) },
            ResultCode::InvalidArgument
        );
        assert_eq!((width, height), (0, 0));
        assert_eq!(bytes, [0x5a; 4]);
    }
    assert_eq!(
        // SAFETY: both initialized outputs and four exclusive bytes are live for this call.
        unsafe { puzzle_surface_snapshot(0, 360, bytes.as_mut_ptr(), 4, &mut width, &mut height) },
        ResultCode::InvalidHandle
    );
    assert_eq!(bytes, [0x5a; 4]);
    assert_eq!(
        // SAFETY: null dimension is intentionally rejected before any dereference.
        unsafe {
            puzzle_surface_snapshot(
                0,
                360,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut height,
            )
        },
        ResultCode::InvalidArgument
    );
}
