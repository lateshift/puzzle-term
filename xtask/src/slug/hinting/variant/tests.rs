//! Executable checks for D-27's specified blending relationships.
//!
//! The rows differ after Slug coverage, so these tests use one fixed
//! fractional alpha to prove the output path itself is distinct. Endpoints
//! are checked because transfer and coverage correction must preserve them.
use super::{Blend, composite, snapped_size};

/// F1's linear interpolation is lighter than the baseline sRGB byte mix.
#[test]
fn linear_light_and_gamma_blending_differ() {
    let gamma = composite(0.5, 255, 0, Blend::Gamma);
    let linear = composite(0.5, 255, 0, Blend::Linear);
    assert_eq!(gamma, 128);
    assert_eq!(linear, 188);
}

/// F2 thickens dark text on white and leaves light-on-dark at F1's result.
#[test]
fn luminance_correction_is_directional() {
    let dark_f1 = composite(0.5, 255, 0, Blend::Linear);
    let dark_f2 = composite(0.5, 255, 0, Blend::Corrected);
    assert!(dark_f2 < dark_f1);
    let light_f1 = composite(0.5, 30, 255, Blend::Linear);
    let light_f2 = composite(0.5, 30, 255, Blend::Corrected);
    assert_eq!(light_f2, light_f1);
}

/// A transparent edge and solid interior retain their supplied colors.
#[test]
fn blend_endpoints_are_stable() {
    for blend in [Blend::Gamma, Blend::Linear, Blend::Corrected] {
        assert_eq!(composite(0.0, 30, 255, blend), 30);
        assert_eq!(composite(1.0, 30, 255, blend), 255);
    }
}

/// An exact 0.73 em cap cannot be snapped at 10 px within the size limit.
#[test]
fn snap_obeys_three_percent_limit() {
    let ten = snapped_size(10, 0.73);
    assert_eq!(ten.size, 10.0);
    assert!(
        ten.refused_percent
            .is_some_and(|value| (value - 4.109589).abs() < 1e-4)
    );
    let thirteen = snapped_size(13, 0.73);
    assert_eq!(thirteen.size, 13.0);
    assert!(
        thirteen
            .refused_percent
            .is_some_and(|value| (value - 5.16333).abs() < 1e-4)
    );
    let twelve = snapped_size(12, 0.73);
    assert!((twelve.size * 0.73 - 9.0).abs() < 1e-5);
    assert!(twelve.refused_percent.is_none());
}
