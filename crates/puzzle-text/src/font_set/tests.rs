//! Real bundled faces exercise variable weight, fallback and D-27 hints.
use super::{FontSet, FontStyle};
use crate::{
    FontData,
    glyph::{GlyphBackend, GlyphKey, GlyphPlacement, HintMode, slug::SlugBackend},
};
/// Cold readiness and reused preparation protect the frame admission seam.
mod preparation;
/// Complete native sources reject provisional results and retain final negative identity.
mod synchronous;

/// Reads the approved bundled face from the workspace test fixture.
fn bundled(path: &str) -> FontData {
    let absolute = format!("{}/../../assets/fonts/{path}", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&absolute).unwrap_or_else(|error| panic!("{absolute}: {error}"));
    FontData::new(bytes, 0)
}

/// Builds one real B placement before any backend prepares its outline.
fn b_fixture() -> (FontSet, GlyphPlacement) {
    let mut fonts = FontSet::new(
        bundled("JetBrainsMono-Variable.ttf"),
        Some(bundled("JetBrainsMono-Italic-Variable.ttf")),
        Some(bundled("SymbolsNerdFontMono-Regular.ttf")),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let face_id = fonts
        .choose_face("B", FontStyle::Regular)
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("bundled regular face lacks B"));
    let glyph_id = fonts
        .face(face_id)
        .and_then(|face| face.glyph_id('B').ok().flatten())
        .unwrap_or_else(|| panic!("bundled regular face lacks B"));
    let placement = GlyphPlacement {
        cell_column: 0,
        key: GlyphKey {
            face: face_id,
            glyph_id,
            instance: 0,
            hint_ppem: 0,
            hint_mode: HintMode::None,
        },
        origin_px: [16, 32],
        baseline_px: 12.0,
        size_px: 12.0,
        advance_px: 8,
        ink_limit_px: 8,
        cell_height_px: 16,
        cell_span: 1,
        color: 0xffff_ffff,
        effect: false,
        color_glyph: false,
    };
    (fonts, placement)
}

/// A ppem-specific shape does not change the position or advance of a B.
#[test]
fn hinted_shape_keeps_grid_placement() {
    let (mut fonts, mut placement) = b_fixture();
    let mut backend = SlugBackend::default();
    backend
        .prepare(&placement, &mut fonts)
        .unwrap_or_else(|error| panic!("unhinted: {error}"));
    let original_origin = placement.origin_px;
    let original_advance = placement.advance_px;
    placement.key.hint_ppem = 12;
    placement.key.hint_mode = HintMode::Light;
    backend
        .prepare(&placement, &mut fonts)
        .unwrap_or_else(|error| panic!("hinted: {error}"));
    assert_eq!(placement.origin_px, original_origin);
    assert_eq!(placement.advance_px, original_advance);
    let at_12 = backend
        .get(placement.key)
        .unwrap_or_else(|| panic!("missing 12 ppem shape"));
    // Reusing the same ppem must not grow the atlas, whereas a new ppem
    // needs fresh fitted control points and a different texture handle.
    let repeated = backend
        .prepare(&placement, &mut fonts)
        .unwrap_or_else(|error| panic!("repeat: {error}"));
    assert!(
        matches!(repeated, crate::glyph::PreparedGlyph::Vector(handle)
        if handle.glyph_loc == at_12.glyph_loc && handle.generation == at_12.generation)
    );
    placement.key.hint_ppem = 13;
    backend
        .prepare(&placement, &mut fonts)
        .unwrap_or_else(|error| panic!("13 ppem: {error}"));
    assert_ne!(
        backend.get(placement.key).map(|handle| handle.glyph_loc),
        Some(at_12.glyph_loc)
    );
    assert_eq!(placement.origin_px, original_origin);
    assert_eq!(placement.advance_px, original_advance);
}

/// The bundled icon face precedes tofu while an unassigned scalar reaches it.
#[test]
fn symbols_fallback_and_tofu_boundary() {
    let mut fonts = FontSet::new(
        bundled("JetBrainsMono-Variable.ttf"),
        None,
        Some(bundled("SymbolsNerdFontMono-Regular.ttf")),
    )
    .unwrap_or_else(|error| panic!("bundled faces: {error}"));
    // U+F015 is the Nerd Fonts home symbol shown in the M4 scenario.
    // JetBrains Mono does not own this icon; the approved symbol face does.
    assert_eq!(fonts.choose_face("", FontStyle::Regular), Ok(Some(4)));
    // U+0378 is permanently unassigned and must not alias a font's .notdef.
    assert_eq!(fonts.choose_face("\u{0378}", FontStyle::Regular), Ok(None));
    // The variable primary uses its real 700 axis, while the fixed
    // Symbols face needs a second ink layer only in bold styles.
    assert!(!fonts.needs_synthetic_bold(FontStyle::Bold, 1));
    assert!(fonts.needs_synthetic_bold(FontStyle::Bold, 4));
    assert!(!fonts.needs_synthetic_bold(FontStyle::Regular, 4));
}

/// Explicit UI glyphs must never become grid fallback candidates.
mod ui_icons;
