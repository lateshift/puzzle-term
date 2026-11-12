//! A thumbnail must preserve orientation, premultiplied bytes and the rendered crop.
#![allow(
    clippy::unwrap_used,
    reason = "fixed valid test extents must produce dimensions"
)]
use super::*;

#[test]
fn snapshot_pixels_keep_crop_rgba_and_premultiplied_edges() {
    // Native padding is deliberately hostile: allocation bytes are not source ink.
    let data = [
        0, 0, 128, 128, 0, 0, 0, 0, 255, 255, 255, 255, 200, 40, 10, 255, 200, 40, 10, 255, 255,
        255, 255, 255,
    ];
    let mut full = [0; 16];
    copy(&data, 12, [2, 2], [2, 2], &mut full);
    assert_eq!(
        full,
        [
            128, 0, 0, 128, 0, 0, 0, 0, 10, 40, 200, 255, 10, 40, 200, 255
        ]
    );
    let mut reduced = [0; 4];
    copy(&data, 12, [2, 2], [1, 1], &mut reduced);
    assert_eq!(reduced, [37, 20, 100, 160]);
    assert_eq!(size([5120, 2880], 360).unwrap(), [360, 202]);
    assert_eq!(size([2, 1], 360).unwrap(), [2, 1]);
    assert_eq!(size([5120, 1], 1).unwrap(), [1, 1]);
    assert!(matches!(size([2, 2], 0), Err(SnapshotError::InvalidWidth)));
    // Sampling is caller-owned output work; no temporary full-resolution mirror.
    let allocations = allocation_counter::measure(|| copy(&data, 12, [2, 2], [1, 1], &mut reduced));
    assert_eq!(allocations.count_total, 0);
}
