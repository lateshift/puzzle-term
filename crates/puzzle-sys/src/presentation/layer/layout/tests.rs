//! Fractional pane dimensions preserve the authoritative physical extent exactly.
use super::*;

/// Odd sizes at 2x are half-points, not permission to enlarge a drawable.
#[test]
fn host_pixel_bounds_preserve_odd_sizes_at_both_scales() {
    for scale in [1., 2.] {
        for size in [[991, 1192], [1001, 777], [1000, 1192]] {
            let wanted = Geometry { size, scale };
            let bounds = host_bounds(wanted);
            assert_eq!(bounds.size.width, f64::from(size[0]) / scale);
            assert_eq!(bounds.size.height, f64::from(size[1]) / scale);
            assert_eq!(bounds.size.width * scale, f64::from(size[0]));
            assert_eq!(bounds.size.height * scale, f64::from(size[1]));
        }
    }
}
