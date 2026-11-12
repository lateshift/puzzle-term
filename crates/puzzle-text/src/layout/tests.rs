//! Real-font tests keep grid placement separate from substitution and hinting.

use super::{LayoutOptions, RowLayout, line};
use crate::{
    FontData,
    builtin::{BUILTIN_FACE, TOFU_FACE},
    font_set::{FontSet, FontStyle},
    hinting::HintingSetting,
    shape::{FeatureSetting, TextShaper},
};

/// Loads the approved face through the same caller-owned byte boundary as the harness.
fn fonts() -> Result<FontSet, String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/JetBrainsMono-Variable.ttf"
    );
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    FontSet::new(FontData::new(bytes, 0), None, None)
}

/// Chooses standard single-row grid options for a reproducible font fixture.
fn options() -> LayoutOptions<'static> {
    LayoutOptions {
        size: 12.0,
        dpr: 1.0,
        origin_px: [8, 4],
        style: FontStyle::Regular,
        hinting: HintingSetting::Auto,
        ligatures: true,
        features: &[],
        cursor_byte: None,
        transformed: false,
        symbol_overflow: true,
        builtin_glyphs: true,
        color: 0xffff_ffff,
    }
}

/// Warm changed rows reuse both source-cluster and neutral-placement space.
#[test]
fn warm_row_layout_allocates_nothing() -> Result<(), String> {
    let mut fonts = fonts()?;
    let mut shaper = TextShaper::default();
    let mut row = RowLayout::default();
    let first = "Hamburgefonstiv 0Oo1lI -> => != ==".repeat(8);
    let second = first.replace('H', "h");
    let first = &first[..240];
    let second = &second[..240];
    // Warm both exact run keys before measuring changed rows. A freshly
    // seen run may own a cache entry; steady rows must reuse allocations.
    row.line(&mut fonts, &mut shaper, first, options())?;
    row.line(&mut fonts, &mut shaper, second, options())?;
    let mut failed = false;
    let allocation = allocation_counter::measure(|| {
        for text in [first, second, first, second] {
            match row.line(&mut fonts, &mut shaper, text, options()) {
                Ok(placed) => {
                    std::hint::black_box(placed.glyphs.len());
                }
                Err(_) => failed = true,
            }
        }
    });
    assert!(!failed, "a warm row failed layout");
    assert_eq!(
        allocation.count_total, 0,
        "warm row allocated: {allocation:?}"
    );
    Ok(())
}

/// Reusing a row must not retain a prior run's glyphs or cell count.
#[test]
fn reused_row_matches_fresh_unicode_and_builtin_layout() -> Result<(), String> {
    let mut fonts = fonts()?;
    let mut shaper = TextShaper::default();
    let mut reused = RowLayout::default();
    // These rows change run count, Unicode byte width and procedural ink.
    // A stale cluster after a shorter row would place a later glyph badly.
    for text in ["Hamburgefonstiv -> !=", "e\u{301}─", "B", "-> !="] {
        let actual = reused.line(&mut fonts, &mut shaper, text, options())?;
        let expected = line(&mut fonts, &mut shaper, text, options())?;
        assert_eq!(actual.metrics, expected.metrics);
        assert_eq!(actual.cells, expected.cells);
        assert_eq!(actual.glyphs, expected.glyphs);
    }
    Ok(())
}

/// Cursor splitting and hint selection cannot change later cell positions.
#[test]
fn cursor_break_and_hinting_preserve_grid() -> Result<(), String> {
    let mut fonts = fonts()?;
    let mut shaper = TextShaper::default();
    let normal = line(&mut fonts, &mut shaper, "->X", options())?;
    let mut cursor = options();
    cursor.cursor_byte = Some(1);
    let broken = line(&mut fonts, &mut shaper, "->X", cursor)?;
    assert_eq!(normal.cells, 3);
    assert_eq!(broken.cells, 3);
    assert_eq!(normal.metrics.width_px, broken.metrics.width_px);
    let mut unhinted = options();
    unhinted.hinting = HintingSetting::None;
    let unhinted = line(&mut fonts, &mut shaper, "->X", unhinted)?;
    assert_eq!(
        normal.glyphs.last().map(|glyph| glyph.origin_px[0]),
        unhinted.glyphs.last().map(|glyph| glyph.origin_px[0])
    );
    assert!(normal.glyphs.iter().all(|glyph| glyph.key.hint_ppem == 12));
    assert!(unhinted.glyphs.iter().all(|glyph| glyph.key.hint_ppem == 0));
    Ok(())
}

/// Procedural box cells use consecutive origins and preserve a tofu fallback.
#[test]
fn builtin_cells_and_tofu_follow_the_grid() -> Result<(), String> {
    let mut fonts = fonts()?;
    let mut shaper = TextShaper::default();
    let layout = line(&mut fonts, &mut shaper, "──\u{10ffff}", options())?;
    assert_eq!(layout.glyphs[0].key.face, BUILTIN_FACE);
    assert_eq!(layout.glyphs[1].key.face, BUILTIN_FACE);
    assert_eq!(layout.glyphs[2].key.face, TOFU_FACE);
    assert_eq!(
        layout.glyphs[1].origin_px[0] - layout.glyphs[0].origin_px[0],
        layout.metrics.width_px as i32
    );
    Ok(())
}

/// D-31: a variation selector shapes with its base and never gets its own box.
#[test]
fn variation_selector_does_not_make_a_second_glyph() -> Result<(), String> {
    let mut fonts = fonts()?;
    let mut shaper = TextShaper::default();
    let plain = line(&mut fonts, &mut shaper, "ab✌cd|", options())?;
    let emoji = line(&mut fonts, &mut shaper, "ab✌️cd|", options())?;
    assert_eq!(plain.glyphs.len(), emoji.glyphs.len());
    assert_eq!(emoji.cells, plain.cells + 1);
    assert_eq!(emoji.glyphs[2].key.face, plain.glyphs[2].key.face);
    // With no pictograph font, both rows use the same one-cell tofu
    // outline; VS16 reserves a blank second cell for placement only.
    assert_eq!(emoji.glyphs[2].key, plain.glyphs[2].key);
    assert_eq!(emoji.glyphs[2].ink_limit_px, plain.glyphs[2].ink_limit_px);
    assert_eq!(
        emoji.glyphs[2].advance_px,
        plain.glyphs[2].advance_px + plain.metrics.width_px
    );
    assert_eq!(
        emoji.glyphs[4].origin_px[0],
        plain.glyphs[4].origin_px[0] + plain.metrics.width_px as i32
    );
    assert_eq!(emoji.glyphs[4].origin_px[1], plain.glyphs[4].origin_px[1]);
    // The font may lack a VS cmap entry, but the selector cannot replace
    // an otherwise covered base with a tofu face.
    let letter = line(&mut fonts, &mut shaper, "x\u{fe0f}", options())?;
    assert_eq!(
        letter.glyphs.len(),
        1,
        "{:?}, vs={:?}",
        letter.glyphs,
        fonts.face(0).map(|face| face.glyph_id('\u{fe0f}'))
    );
    assert_ne!(letter.glyphs[0].key.face, TOFU_FACE);
    let orphan = line(&mut fonts, &mut shaper, "\u{fe0f}\u{200d}", options())?;
    assert!(orphan.glyphs.is_empty());
    Ok(())
}

/// A Nerd icon may use a blank neighbour without moving the later cell.
#[test]
fn symbol_overflow_requires_a_blank_neighbour() -> Result<(), String> {
    let symbols = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/SymbolsNerdFontMono-Regular.ttf"
    );
    let bytes = std::fs::read(symbols).map_err(|error| error.to_string())?;
    let regular = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/JetBrainsMono-Variable.ttf"
    ))
    .map_err(|error| error.to_string())?;
    let mut fonts = FontSet::new(
        FontData::new(regular, 0),
        None,
        Some(FontData::new(bytes, 0)),
    )?;
    let mut shaper = TextShaper::default();
    let blank = line(&mut fonts, &mut shaper, " X", options())?;
    let occupied = line(&mut fonts, &mut shaper, "X", options())?;
    let cell = blank.metrics.width_px;
    assert_eq!(blank.glyphs[0].ink_limit_px, cell * 2);
    assert_eq!(occupied.glyphs[0].ink_limit_px, cell);
    assert_eq!(blank.glyphs[0].advance_px, occupied.glyphs[0].advance_px);
    assert_eq!(
        occupied.glyphs[1].origin_px[0] - occupied.glyphs[0].origin_px[0],
        cell as i32
    );
    Ok(())
}

/// User feature overrides reach Harfrust and builtin glyphs can be disabled.
#[test]
fn layout_respects_font_feature_and_builtin_choices() -> Result<(), String> {
    let mut fonts = fonts()?;
    let mut shaper = TextShaper::default();
    let default = line(&mut fonts, &mut shaper, "->", options())?;
    // A layout feature override must survive the run boundary; otherwise
    // the settings value would be accepted without changing glyphs.
    let disabled = [
        FeatureSetting {
            tag: *b"liga",
            value: 0,
        },
        FeatureSetting {
            tag: *b"calt",
            value: 0,
        },
        FeatureSetting {
            tag: *b"dlig",
            value: 0,
        },
    ];
    let mut plain = options();
    plain.features = &disabled;
    let unligated = line(&mut fonts, &mut shaper, "->", plain)?;
    assert_ne!(
        default
            .glyphs
            .iter()
            .map(|glyph| glyph.key.glyph_id)
            .collect::<Vec<_>>(),
        unligated
            .glyphs
            .iter()
            .map(|glyph| glyph.key.glyph_id)
            .collect::<Vec<_>>()
    );
    let procedural = line(&mut fonts, &mut shaper, "─", options())?;
    plain.builtin_glyphs = false;
    let font_drawn = line(&mut fonts, &mut shaper, "─", plain)?;
    assert_eq!(procedural.glyphs[0].key.face, BUILTIN_FACE);
    assert_ne!(font_drawn.glyphs[0].key.face, BUILTIN_FACE);
    Ok(())
}

/// Blank source eligibility must stop before platform ink discovery.
mod blank;
