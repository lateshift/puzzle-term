//! Natural annotation advances, explicit fallback faces and COLRv1 source identities.
#![allow(clippy::expect_used, reason = "fixture failures are test assertions")]
use super::*;
use crate::{FontData, font_set::FontStyle, glyph::slug::SlugBackend};
/// Explicit font selection must win over supplementary Nerd Font PUA collisions.
#[test]
fn explicit_colr_face_shapes_and_prepares_real_color_layers() {
    let primary = FontData::new(
        include_bytes!("../../../../../assets/fonts/JetBrainsMono-Variable.ttf").to_vec(),
        0,
    );
    let symbols = FontData::new(
        include_bytes!("../../../../../assets/fonts/SymbolsNerdFontMono-Regular.ttf").to_vec(),
        0,
    );
    let mut fonts = FontSet::new(primary, None, Some(symbols)).expect("fonts");
    let color = fonts
        .add_fallback(FontData::new(
            include_bytes!("../../../../../assets/fonts/test/test_glyphs-glyf_colr_1.ttf").to_vec(),
            0,
        ))
        .expect("color");
    let mut shaper = TextShaper::default();
    let mut layouter = Layouter::default();
    let output = layouter
        .line(
            &mut fonts,
            &mut shaper,
            "\u{f0100}\u{f0200}",
            options(),
            Some(color),
        )
        .expect("layout");
    assert_eq!(
        output
            .glyphs
            .iter()
            .map(|p| (p.key.face, p.key.glyph_id))
            .collect::<Vec<_>>(),
        [(color, 8), (color, 12)]
    );
    assert!(output.width > 80.0, "natural advance {:?}", output.width);
    let mut backend = SlugBackend::default();
    for p in &output.glyphs {
        let layers = backend
            .prepare_color(p, &fonts)
            .expect("color preparation")
            .expect("COLR");
        assert!(
            !layers.layers.is_empty(),
            "glyph {:?} contains no color layers",
            p
        );
    }
}
/// Effects use the exact requested size and never introduce ppem hinting.
fn options() -> LayoutOptions<'static> {
    LayoutOptions {
        size: 45.0,
        dpr: 1.0,
        origin_px: [0; 2],
        style: FontStyle::Regular,
        hinting: crate::hinting::HintingSetting::None,
        ligatures: true,
        features: &[],
        cursor_byte: None,
        transformed: true,
        symbol_overflow: false,
        builtin_glyphs: false,
        color: 0xffffffff,
    }
}
