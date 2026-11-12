//! Whole-cluster source ordering and grid invariance (M11.1, D-31).
#![allow(
    clippy::unwrap_used,
    reason = "fixture absence and contract failures must fail these tests"
)]
use super::*;
use crate::{
    cluster_source::*,
    glyph::GlyphPlacement,
    hinting::HintingSetting,
    layout::{self, LayoutOptions},
    presentation::{Presentation, presentation},
    shape::TextShaper,
};
use kurbo::Shape;

/// Fake coverage intentionally includes controls to catch accidental lookups.
struct Fake;
impl EmojiSource for Fake {
    fn can_draw(&mut self, request: ClusterRequest<'_>) -> bool {
        assert!(!request.cluster.chars().all(is_default_ignorable));
        true
    }
    fn prepare(
        &mut self,
        _: ClusterRequest<'_>,
        _: &GlyphPlacement,
    ) -> Result<ClusterGlyph, String> {
        Ok(ClusterGlyph::Outline(
            kurbo::Rect::new(0.0, 0.0, 1.0, 1.0).to_path(0.001),
        ))
    }
}
impl PlatformFallback for Fake {
    fn can_draw(&mut self, request: ClusterRequest<'_>) -> bool {
        assert!(!request.cluster.chars().all(is_default_ignorable));
        true
    }
    fn prepare(
        &mut self,
        _: ClusterRequest<'_>,
        _: &GlyphPlacement,
    ) -> Result<ClusterGlyph, String> {
        Ok(ClusterGlyph::Outline(
            kurbo::Rect::new(0.0, 0.0, 1.0, 1.0).to_path(0.001),
        ))
    }
}

/// Loads approved fixture bytes, with no dependency on system font installation.
fn fonts() -> FontSet {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/JetBrainsMono-Variable.ttf"
    ))
    .unwrap();
    FontSet::new(FontData::new(bytes, 0), None, None).unwrap()
}

/// Scalar source identities must survive complete repeated Unicode benchmark cycles.
#[test]
fn unicode_source_keys_survive_metadata_pressure() {
    let mut fonts = fonts();
    let initial = fonts
        .source_key("漢", FontStyle::Regular, PLATFORM_FACE)
        .unwrap();
    let mut bytes = [0; 4];
    for scalar in 0x4e00..0x7200 {
        let cluster = char::from_u32(scalar).unwrap().encode_utf8(&mut bytes);
        fonts
            .source_key(cluster, FontStyle::Regular, PLATFORM_FACE)
            .unwrap();
    }
    // Composed clusters still use the bounded string registry; retire its original
    // entries too, proving the scalar namespace remains independent of that pressure.
    for scalar in 0x4e00..0x6200 {
        let cluster = format!("{}\u{301}", char::from_u32(scalar).unwrap());
        fonts
            .source_key(&cluster, FontStyle::Regular, PLATFORM_FACE)
            .unwrap();
    }
    assert_eq!(
        fonts
            .source_key("漢", FontStyle::Regular, PLATFORM_FACE)
            .unwrap(),
        initial,
        "Unicode pressure assigned a new key to already prepared scalar ink"
    );
}

/// Style and selectors never disappear from a source request or its identity.
#[test]
fn presentation_source_table() {
    let mut fonts = fonts();
    fonts.set_emoji_source(Box::new(Fake));
    fonts.set_platform_fallback(Box::new(Fake));
    for (text, expected) in [
        ("🦀", Presentation::Emoji),
        ("⌚", Presentation::Emoji),
        ("✌️", Presentation::Emoji),
        ("☺️", Presentation::Emoji),
        ("✌︎", Presentation::Text),
        ("⌚︎", Presentation::Text),
        ("1️⃣", Presentation::Emoji),
        ("👩‍💻", Presentation::Emoji),
        ("🇩🇪", Presentation::Emoji),
        ("👍🏽", Presentation::Emoji),
        ("☺", Presentation::Text),
        ("✌", Presentation::Text),
        ("漢", Presentation::Text),
    ] {
        for style in [
            FontStyle::Regular,
            FontStyle::Bold,
            FontStyle::Italic,
            FontStyle::BoldItalic,
        ] {
            assert_eq!(presentation(text), expected, "{text}");
            let face = fonts.choose_face(text, style).unwrap().unwrap();
            if expected == Presentation::Emoji {
                assert_eq!(face, EMOJI_FACE, "{text}");
            } else {
                assert_ne!(face, EMOJI_FACE, "{text}");
            }
            // The cache distinguishes selector appearance for one base scalar.
            let first = text.chars().next().unwrap();
            assert_eq!(
                fonts.selection_cache.get(&(first, style, expected)),
                Some(&face)
            );
        }
    }
    for invisible in ["\u{fe0f}", "\u{200d}", "\u{e0100}", "\u{2060}"] {
        assert_eq!(
            fonts.choose_face(invisible, FontStyle::Regular).unwrap(),
            None
        );
    }
}

/// Subsequent cells and source spans are identical with fake sources and tofu.
#[test]
fn source_cell_positions_table() {
    let mut plain = fonts();
    let mut supplied = fonts();
    supplied.set_emoji_source(Box::new(Fake));
    supplied.set_platform_fallback(Box::new(Fake));
    for (text, width) in [
        ("🦀", 2),
        ("✌️", 2),
        ("✌︎", 1),
        ("☺", 1),
        ("1️⃣", 2),
        ("👩‍💻", 2),
        ("🇩🇪", 2),
        ("👍🏽", 2),
        ("漢", 2),
        ("⌚\u{fe0e}\u{fe0f}", 2),
        ("x\u{fe0f}\u{fe0f}", 1),
        ("1\u{20e3}", 1),
        ("🏴\u{200d}☠️", 2),
    ] {
        for dpr in [1.0, 2.0] {
            let options = LayoutOptions {
                size: 14.0,
                dpr,
                origin_px: [20, 0],
                style: FontStyle::Regular,
                hinting: HintingSetting::None,
                ligatures: false,
                features: &[],
                cursor_byte: None,
                transformed: false,
                symbol_overflow: false,
                builtin_glyphs: false,
                color: 0xffffffff,
            };
            let line = format!("{text}A");
            let before =
                layout::line(&mut plain, &mut TextShaper::default(), &line, options).unwrap();
            let after =
                layout::line(&mut supplied, &mut TextShaper::default(), &line, options).unwrap();
            assert_eq!(after.cells, width + 1, "{text}");
            assert_eq!(before.cells, after.cells);
            assert_eq!(
                before.glyphs.last().unwrap().origin_px,
                after.glyphs.last().unwrap().origin_px
            );
            assert_eq!(after.glyphs[0].cell_span, width as u8);
            let next_x = 20 + (width * after.metrics.width_px) as i32;
            assert_eq!(after.glyphs.last().unwrap().origin_px[0], next_x);
            if after.glyphs[0].key.face == EMOJI_FACE {
                assert!(matches!(
                    supplied.prepare_source(&after.glyphs[0]).unwrap(),
                    ClusterGlyph::Outline(_)
                ));
            }
        }
    }
}

/// Adding an earlier fallback discards only the source decisions it could change.
#[test]
fn new_face_invalidates_only_affected_choices() {
    let mut fonts = fonts();
    fonts.set_emoji_source(Box::new(Fake));
    fonts.set_platform_fallback(Box::new(Fake));
    for text in ["A", "漢", "🦀"] {
        fonts.choose_face(text, FontStyle::Regular).unwrap();
    }
    let old = fonts.selection_cache.clone();
    fonts.choose_face("\u{f015}", FontStyle::Regular).unwrap();
    assert_eq!(
        fonts
            .selection_cache
            .get(&('\u{f015}', FontStyle::Regular, Presentation::Text)),
        Some(&PLATFORM_FACE)
    );
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/SymbolsNerdFontMono-Regular.ttf"
    ))
    .unwrap();
    fonts.add_fallback(FontData::new(bytes, 0)).unwrap();
    // These old choices are primary, uncovered CJK and emoji-first respectively.
    assert_eq!(fonts.selection_cache, old);
    assert_ne!(
        fonts.choose_face("\u{f015}", FontStyle::Regular).unwrap(),
        Some(PLATFORM_FACE)
    );
}

/// The complete VT parity cases also cover multiple graphemes and unattached marks.
#[test]
fn d31_width_parity_table() {
    // Source: reference cursor-position-report measurements, 2026-09-30;
    // kept identical to puzzle-vt/tests/conformance/unicode.rs.
    for (text, cells) in [
        ("❤️", 2),
        ("x\u{fe0f}\u{fe0f}", 1),
        ("⌚\u{fe0e}\u{fe0f}", 2),
        ("🏴\u{200d}☠️", 2),
        ("👨\u{200d}👩\u{200d}👧", 2),
        ("#️⃣", 2),
        ("1\u{20e3}", 1),
        ("👋🏿", 2),
        ("🇦🇧🇨", 4),
        ("ab", 2),
        ("\u{0301}\u{0302}", 0),
        ("✌️", 2),
        ("✌", 1),
    ] {
        assert_eq!(layout::cell_count(text).unwrap(), cells, "{text}");
    }
}
