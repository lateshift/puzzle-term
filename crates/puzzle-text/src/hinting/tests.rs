//! The D-27 threshold and effect rules are not visual guesses (§9.7).
use super::{HintingSetting, choose};
use crate::glyph::HintMode;

/// Light auto-hinting is confined to untransformed grid text below 1.5 DPR.
#[test]
fn eligibility_boundaries() {
    let low =
        choose(HintingSetting::Auto, 13.0, 1.0, false).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!((low.mode, low.ppem), (HintMode::Light, 13));
    for (dpr, transformed) in [(1.5, false), (2.0, false), (1.0, true)] {
        let choice = choose(HintingSetting::Auto, 13.0 * dpr, dpr, transformed)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!((choice.mode, choice.ppem), (HintMode::None, 0));
    }
}

/// Explicit native mode changes only the cache/outline choice, not size.
#[test]
fn native_and_invalid_sizes() {
    let choice =
        choose(HintingSetting::Native, 12.0, 1.0, false).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!((choice.mode, choice.ppem), (HintMode::Native, 12));
    assert!(choose(HintingSetting::Light, f32::NAN, 1.0, false).is_err());
    assert!(choose(HintingSetting::Light, 12.0, 0.0, false).is_err());
}
