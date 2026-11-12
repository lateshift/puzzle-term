//! Cold faces and hint engines are never admitted solely because time is available.
use crate::{
    glyph::HintMode,
    preparation::{Ink, Job, faces::Faces},
};
use std::time::Duration;

/// A worker result is the original drawn geometry, and first-use hint setup stays cold.
#[test]
fn parsed_face_and_hinter_are_required_before_inline_work() {
    let (mut fonts, placement) = super::b_fixture();
    let face = fonts
        .face(placement.key.face)
        .unwrap_or_else(|| panic!("face absent"));
    let mut job = Job {
        placement,
        data: Some(face.data()),
        weight: face.weight(),
        style: [false; 3],
    };
    let mut loaded = Faces::default();
    // A large allowance must not make unopened tables eligible for inline preparation.
    assert!(loaded.predicted(&job).is_none());
    loaded.load(&job).unwrap_or_else(|error| panic!("{error}"));
    assert!(loaded.predicted(&job).is_none());
    let Some(Ink::Outline(prepared)) = loaded
        .prepare(&job)
        .unwrap_or_else(|error| panic!("{error}"))
    else {
        panic!("B has no outline")
    };
    use crate::glyph::GlyphOutlineSource;
    assert_eq!(
        prepared.path,
        fonts
            .outline(placement.key)
            .unwrap_or_else(|error| panic!("{error}"))
    );
    assert!(!prepared.shape.contours.is_empty());
    loaded.record(&job, Duration::from_micros(20));
    assert_eq!(loaded.predicted(&job), Some(Duration::from_micros(20)));
    // A new device size must initialize its hinter off-frame, even for a resident face.
    job.placement.key.hint_ppem = 12;
    job.placement.key.hint_mode = HintMode::Light;
    assert!(loaded.predicted(&job).is_none());
    assert!(matches!(
        loaded
            .prepare(&job)
            .unwrap_or_else(|error| panic!("{error}")),
        Some(Ink::Outline(_))
    ));
    // Parsed hint state cannot borrow the unhinted kind's cost evidence.
    assert!(loaded.predicted(&job).is_none());
    loaded.record(&job, Duration::from_micros(40));
    assert_eq!(loaded.predicted(&job), Some(Duration::from_micros(40)));
    job.placement.key.hint_ppem = 13;
    assert!(loaded.predicted(&job).is_none());
}

/// Diagnostic components share admission and preserve their complete cell coordinates.
#[test]
fn procedural_component_uses_measured_preparation() {
    let (_, mut placement) = super::b_fixture();
    let cell =
        crate::builtin::BuiltinCell::new(12, 20, 1).unwrap_or_else(|error| panic!("{error}"));
    placement.key.face = crate::builtin::TOFU_COMPONENT_FACE;
    placement.key.instance = cell.pack().unwrap_or_else(|error| panic!("{error}"));
    placement.key.glyph_id = 18;
    let job = Job {
        placement,
        data: None,
        weight: 400,
        style: [false; 3],
    };
    let mut loaded = Faces::default();
    assert!(loaded.predicted(&job).is_none());
    let Some(Ink::Outline(ink)) = loaded
        .prepare(&job)
        .unwrap_or_else(|error| panic!("{error}"))
    else {
        panic!("diagnostic absent")
    };
    assert_eq!(
        ink.path,
        cell.tofu_component(18)
            .unwrap_or_else(|error| panic!("{error}"))
            .path
    );
    loaded.record(&job, Duration::from_micros(3));
    assert!(loaded.predicted(&job).is_some());
}
