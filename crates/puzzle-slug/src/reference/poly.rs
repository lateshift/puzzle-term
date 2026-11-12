//! Direct Rust translation of the reference shader's root calculations.
//!
//! This module is part of the licensed reference port credited in `reference.rs`.
//! Source: SlugPixelShader.hlsl at be3c13eb7d63f9e8aa5c583e42d92c374cb91d98.
//! Float operations and the 0x2E74 / 1/65536 constants stay in reference
//! order so M3's WGSL implementation can be compared line by line.

/// Calculate the root eligibility code for a sample-relative quadratic Bézier curve.
/// Extract the signs of the y coordinates of the three control points.
#[must_use]
pub(super) fn calc_root_code(y1: f32, y2: f32, y3: f32) -> u32 {
    let i1 = y1.to_bits() >> 31;
    let i2 = y2.to_bits() >> 30;
    let i3 = y3.to_bits() >> 29;
    let mut shift = (i2 & 2) | (i1 & !2);
    shift = (i3 & 4) | (shift & !4);
    // Eligibility is returned in bits 0 and 8.
    (0x2E74_u32 >> shift) & 0x0101
}

/// Solve for the values of t where the curve crosses y = 0.
///
/// The polynomial is a t² − 2b t + c, with a = p1 − 2p2 + p3,
/// b = p1 − p2 and c = p1. Imaginary roots become a double root at
/// the global minimum t = b/a, exactly as in the HLSL reference.
#[must_use]
pub(super) fn solve_horiz_poly(p12: [f32; 4], p3: [f32; 2]) -> [f32; 2] {
    let a = [p12[0] - p12[2] * 2.0 + p3[0], p12[1] - p12[3] * 2.0 + p3[1]];
    let b = [p12[0] - p12[2], p12[1] - p12[3]];
    let ra = 1.0 / a[1];
    let rb = 0.5 / b[1];
    let d = (b[1] * b[1] - a[1] * p12[1]).max(0.0).sqrt();
    let mut t1 = (b[1] - d) * ra;
    let mut t2 = (b[1] + d) * ra;
    // If the polynomial is nearly linear, solve −2b t + c = 0.
    if a[1].abs() < 1.0 / 65536.0 {
        t1 = p12[1] * rb;
        t2 = t1;
    }
    // Return the x coordinates where C(t) = 0.
    [
        (a[0] * t1 - b[0] * 2.0) * t1 + p12[0],
        (a[0] * t2 - b[0] * 2.0) * t2 + p12[0],
    ]
}

/// Solve for the values of t where the curve crosses x = 0.
#[must_use]
pub(super) fn solve_vert_poly(p12: [f32; 4], p3: [f32; 2]) -> [f32; 2] {
    let a = [p12[0] - p12[2] * 2.0 + p3[0], p12[1] - p12[3] * 2.0 + p3[1]];
    let b = [p12[0] - p12[2], p12[1] - p12[3]];
    let ra = 1.0 / a[0];
    let rb = 0.5 / b[0];
    let d = (b[0] * b[0] - a[0] * p12[0]).max(0.0).sqrt();
    let mut t1 = (b[0] - d) * ra;
    let mut t2 = (b[0] + d) * ra;
    // If the polynomial is nearly linear, solve −2b t + c = 0.
    if a[0].abs() < 1.0 / 65536.0 {
        t1 = p12[0] * rb;
        t2 = t1;
    }
    // Return the y coordinates where C(t) = 0.
    [
        (a[1] * t1 - b[1] * 2.0) * t1 + p12[1],
        (a[1] * t2 - b[1] * 2.0) * t2 + p12[1],
    ]
}
