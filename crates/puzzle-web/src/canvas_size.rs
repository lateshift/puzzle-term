//! Device-pixel sizing for the browser's fixed M3 review scene.
//!
//! The scene has a logical size, but WebGPU renders into device pixels.
//! Keeping the CSS size equal to the backing size divided by DPR prevents
//! the browser from resampling a completed Slug image during presentation.
//! Fractional DPRs require a whole-pixel backing and possibly fractional
//! CSS dimensions; preserving that fraction avoids a second rounding step.

/// Sizes used together for scene scaling, canvas attributes and CSS.
pub(crate) struct CanvasSize {
    /// Browser device pixels per CSS pixel, or the explicit review override.
    pub(crate) dpr: f64,
    /// Canvas width in device pixels.
    pub(crate) backing_width: u32,
    /// Canvas height in device pixels.
    pub(crate) backing_height: u32,
    /// Canvas width in CSS pixels.
    pub(crate) css_width: f64,
    /// Canvas height in CSS pixels.
    pub(crate) css_height: f64,
}

impl CanvasSize {
    /// Uses a scene that already rounded each grid cell in device pixels.
    #[cfg(any(feature = "review-scenes", test))]
    pub(crate) fn from_backing(width: u32, height: u32, dpr: f64) -> Result<Self, String> {
        // Pack text must not be scaled after cell rounding. Keeping its
        // backing pixels exactly also preserves the full-font takeover grid.
        if !dpr.is_finite() || dpr <= 0.0 || width == 0 || height == 0 {
            return Err("invalid pre-sized canvas or DPR".into());
        }
        Ok(Self {
            dpr,
            backing_width: width,
            backing_height: height,
            css_width: f64::from(width) / dpr,
            css_height: f64::from(height) / dpr,
        })
    }

    /// Rounds the target to whole device pixels, then derives its CSS size.
    pub(crate) fn new(width: u32, height: u32, dpr: f64) -> Result<Self, String> {
        // Browser DPR can change with zoom or monitor placement. The query
        // override is untrusted as well, so neither may enter GPU sizing as
        // zero, NaN or infinity.
        if !dpr.is_finite() || dpr <= 0.0 {
            return Err("DPR must be a finite positive number".into());
        }
        // Reject impossible canvas attributes before converting float to
        // integer, because Rust's saturating cast would hide an overflow.
        let scaled_width = f64::from(width) * dpr;
        let scaled_height = f64::from(height) * dpr;
        if !valid_extent(scaled_width) || !valid_extent(scaled_height) {
            return Err("DPR produces an invalid canvas backing size".into());
        }
        let backing_width = scaled_width.round() as u32;
        let backing_height = scaled_height.round() as u32;
        // Derive the displayed size from the actual backing dimensions,
        // rather than using the logical size, when fractional DPR rounding
        // has changed the number of available device pixels.
        Ok(Self {
            dpr,
            backing_width,
            backing_height,
            css_width: f64::from(backing_width) / dpr,
            css_height: f64::from(backing_height) / dpr,
        })
    }
}

/// Checks that rounding will produce a nonzero canvas attribute in range.
fn valid_extent(value: f64) -> bool {
    // A rounded value below one cannot back a render surface. The upper
    // bound avoids a saturating float-to-u32 conversion at extreme DPRs.
    value.is_finite() && value >= 0.5 && value < f64::from(u32::MAX)
}

#[cfg(test)]
/// Browser-free checks of the values written to canvas and CSS.
mod tests {
    use super::CanvasSize;

    /// Proves the browser's backing and CSS dimensions agree at common DPRs.
    #[test]
    fn canvas_size_invariant() {
        // Fractional values model common Windows scaling; whole values
        // cover the native reference and a typical Retina backing store.
        for (dpr, expected_width) in [(1.0, 248), (1.25, 310), (1.5, 372), (2.0, 496)] {
            let Ok(size) = CanvasSize::new(248, 248, dpr) else {
                panic!("review DPR must produce a valid canvas size");
            };
            assert_eq!(size.dpr, dpr);
            assert_eq!(size.backing_width, expected_width);
            assert_eq!(size.backing_height, expected_width);
            assert_eq!((size.css_width * dpr).round() as u32, size.backing_width);
            assert_eq!((size.css_height * dpr).round() as u32, size.backing_height);
        }
        assert!(CanvasSize::new(248, 248, f64::NAN).is_err());
        assert!(CanvasSize::new(248, 248, 0.0).is_err());
    }

    /// Pack text keeps its per-cell device rounding through CSS sizing.
    #[test]
    fn pack_backing_is_not_rescaled() {
        for dpr in [1.0, 1.25, 1.5, 2.0] {
            let size = CanvasSize::from_backing(416, 180, dpr)
                .unwrap_or_else(|error| panic!("pack canvas: {error}"));
            assert_eq!(size.backing_width, 416);
            assert_eq!(size.backing_height, 180);
            assert_eq!((size.css_width * dpr).round() as u32, 416);
            assert_eq!((size.css_height * dpr).round() as u32, 180);
        }
    }
}
