//! Divan profiles for M4 shaping and prebuilt font-pack startup (§8.4, §8.8).
//!
//! The benchmark binary reads the approved assets at startup. No font bytes
//! are embedded in a shipped neutral crate. The 240-cell row result times
//! `RowLayout::line`: grapheme collection, fallback, a warmed exact-run
//! cache and placement in caller-reused storage. It skips Harfrust during
//! a timed hit, yet it is far more than a cache lookup. Three other benches
//! isolate the user-approved shaping-stage budgets.
//!
//! Each stage benchmark supplies the same 240 ASCII bytes to the same
//! JetBrains Mono face. Only the ligature flag and shaper cache state
//! differ. This removes face coverage and string length as explanations
//! for timing differences. Font loading is outside every timed closure;
//! the full-layout benchmark still includes its own cell metrics and
//! fallback walk because those are part of a changed row's CPU work.
//! The pack bench decodes the full ready-to-upload planes on every sample;
//! the native pack probe separately verifies that those planes produce ink.
//! This split avoids mistaking a fast parser for a successful GPU upload.
//! Both measurements use the exact committed asset bytes so a generator
//! change is reflected in the next benchmark, not hidden by a mock pack.
use puzzle_text::{
    FontData,
    font_set::{FontSet, FontStyle},
    hinting::HintingSetting,
    layout::{LayoutOptions, RowLayout},
    pack::FontPack,
    shape::{ShapingOptions, TextShaper},
};

/// Runs the native divan registry.
fn main() {
    divan::main();
}

/// Loads the bundled font outside the row's timed loop.
fn fonts() -> FontSet {
    // Disk I/O and face construction happen before divan starts timing a
    // row, matching the already-loaded font in a running terminal.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/JetBrainsMono-Variable.ttf"
    );
    let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("{path}: {error}"));
    FontSet::new(FontData::new(bytes, 0), None, None)
        .unwrap_or_else(|error| panic!("bundled benchmark face: {error}"))
}

/// Reuses the same 1× hinted grid geometry for every shaping iteration.
fn options() -> LayoutOptions<'static> {
    // One integer device-pixel origin makes the measured placements
    // comparable with harness text at 12 px on a 1× display.
    LayoutOptions {
        size: 12.0,
        dpr: 1.0,
        origin_px: [0, 0],
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

/// Repeats a coding-ligature phrase to exactly 240 ASCII source cells.
fn row() -> String {
    // ASCII bytes and terminal cells agree here. A whole-row input keeps
    // hash and buffer work representative without involving fallback.
    let source = "Hamburgefonstiv 0Oo1lI -> => != ==".repeat(8);
    // The final repetition is cut at a byte boundary that is also a
    // character boundary: every source scalar in this fixture is ASCII.
    source[..240].to_owned()
}

/// Selects the same default feature set for all three shaping paths.
fn shaping_options(ligatures: bool) -> ShapingOptions<'static> {
    // An empty override list leaves the face's `liga` and `calt` rules
    // intact when enabled; it does not substitute a custom feature plan.
    ShapingOptions {
        ligatures,
        features: &[],
    }
}

/// Times a warmed full row's clustering, fallback and placement cost.
#[divan::bench]
fn cached_240_cell_row(bencher: divan::Bencher) {
    let mut fonts = fonts();
    let mut shaper = TextShaper::default();
    // The repeated phrase contains spaces, but those spaces still enter
    // grapheme collection and advance calculation in `RowLayout::line`.
    let row = row();
    // Populate the plan and exact result before divan's samples; a miss
    // would measure first-use work instead of P4's dirty-row path.
    let mut storage = RowLayout::default();
    if let Err(error) = storage.line(&mut fonts, &mut shaper, &row, options()) {
        panic!("benchmark warmup failed: {error}");
    }
    bencher.bench_local(|| {
        divan::black_box(
            storage
                .line(&mut fonts, &mut shaper, &row, options())
                .map(|placed| placed.glyphs.len()),
        )
    });
}

/// Measures direct ASCII cmap mapping plus first cache insertion.
#[divan::bench]
fn ascii_fast_240_cell_shape(bencher: divan::Bencher) {
    // This isolates `TextShaper::shape`; cell metrics, fallback and
    // placement are measured by the separate full-layout diagnostic.
    let fonts = fonts();
    let face_id = fonts.primary_face(FontStyle::Regular);
    let face = fonts
        .face(face_id)
        .unwrap_or_else(|| panic!("regular face missing"));
    let row = row();
    let settings = shaping_options(false);
    // Disabled coding substitutions force direct cmap lookup. A fresh
    // shaper per sample avoids accidentally timing a cache hit.
    assert!(TextShaper::ascii_fast_path(face, &row, settings).unwrap_or(false));
    if let Err(error) = TextShaper::default().shape(face_id, face, &row, settings) {
        panic!("ASCII benchmark source failed: {error}");
    }
    // Fresh state makes the timed result include key construction and
    // insertion, as a new ASCII run would in a terminal's row cache.
    bencher.bench_local(|| {
        let mut shaper = TextShaper::default();
        divan::black_box(
            shaper
                .shape(face_id, face, &row, settings)
                .map(|glyphs| glyphs.len()),
        )
    });
}

/// Measures exact-key lookup of an already shaped ligature row.
#[divan::bench]
fn cached_240_cell_shape(bencher: divan::Bencher) {
    // This is the narrow interpretation of a shaping cache hit: no
    // grapheme collection, fallback lookup or placement allocation.
    let fonts = fonts();
    let face_id = fonts.primary_face(FontStyle::Regular);
    let face = fonts
        .face(face_id)
        .unwrap_or_else(|| panic!("regular face missing"));
    let row = row();
    let settings = shaping_options(true);
    let mut shaper = TextShaper::default();
    // Only warmup reaches Harfrust. Timed calls hash the row, compare its
    // full key and borrow the 240-cell result without preparing a new run.
    if let Err(error) = shaper.shape(face_id, face, &row, settings) {
        panic!("cache warmup failed: {error}");
    }
    // The cache key still hashes and compares all 240 bytes to reject
    // collisions; the borrowed glyph slice requires no new owned result.
    bencher.bench_local(|| {
        divan::black_box(
            shaper
                .shape(face_id, face, &row, settings)
                .map(|glyphs| glyphs.len()),
        )
    });
}

/// Measures uncached coding substitutions through Harfrust.
#[divan::bench]
fn harfrust_uncached_240_cell_shape(bencher: divan::Bencher) {
    // The face owns parsed OpenType data before the clock begins, matching
    // a loaded terminal font with a newly encountered source run.
    let fonts = fonts();
    let face_id = fonts.primary_face(FontStyle::Regular);
    let face = fonts
        .face(face_id)
        .unwrap_or_else(|| panic!("regular face missing"));
    let row = row();
    let settings = shaping_options(true);
    // A true substitution-capable face is necessary: another font might
    // silently route the same ASCII input through direct cmap lookup.
    assert!(!TextShaper::ascii_fast_path(face, &row, settings).unwrap_or(true));
    if let Err(error) = TextShaper::default().shape(face_id, face, &row, settings) {
        panic!("Harfrust benchmark source failed: {error}");
    }
    // A new shaper prevents this repeated fixture from becoming a hit.
    // The timed call includes buffer population and result insertion.
    bencher.bench_local(|| {
        let mut shaper = TextShaper::default();
        divan::black_box(
            shaper
                .shape(face_id, face, &row, settings)
                .map(|glyphs| glyphs.len()),
        )
    });
}

/// Measures full pack decode rather than a cached directory lookup.
#[divan::bench]
fn regular_pack_decode(bencher: divan::Bencher) {
    // This pack is unhinted and size-independent; the D-27 hinted shapes
    // are deliberately absent and prepared later when the TTF arrives.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fontpacks/jetbrains-regular.pzfp"
    );
    let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("{path}: {error}"));
    // A fresh BTreeMap and both texel planes are reconstructed each time;
    // the source TTF is absent from this benchmark's data path.
    bencher.bench_local(|| divan::black_box(FontPack::decode(&bytes)));
}
