//! Real JetBrains Mono glyph preparation budget (design §15.2, M2).
//!
//! Font decoding happens outside the timed loop. Each iteration measures the
//! specified outline-to-bands-to-texels work on a warm CPU mirror, without
//! letting the cache turn the operation into a handle lookup.
//!
//! The D-27 hinted benchmark has a different scope: the interpreter runs
//! while skrifa draws an outline at a specific ppem, so its timed body
//! includes that draw and the later Slug preparation. The original M2
//! figure starts from an already extracted path and remains the §15 budget
//! measurement. Report both scopes explicitly instead of subtracting
//! guessed interpreter time from the larger number.
//!
//! The same uppercase B is used in both paths because its two counters
//! create several band memberships and curved segments. A rectangle would
//! understate the conversion and packing work of ordinary terminal text.

/// Runs divan's native benchmark registry.
fn main() {
    divan::main();
}

/// Measures one representative curved capital from the bundled face.
#[divan::bench]
fn jetbrains_capital_b(bencher: divan::Bencher) {
    // Embedding the approved font removes disk I/O and cache state from
    // the measured outline-to-texels operation.
    let bytes = include_bytes!("../../../assets/fonts/JetBrainsMono-Variable.ttf");
    let font = puzzle_text::probe::ProbeFont::new(bytes);
    let path = match font.and_then(|face| face.glyph('B')) {
        Ok((path, _)) => path,
        Err(error) => panic!("bundled benchmark font failed: {error}"),
    };
    let mut atlas = puzzle_slug::SlugAtlas::default();
    // The atlas itself is warmed outside divan's repetitions; reset keeps
    // each repetition on preparation rather than a cached shape lookup.
    bencher.bench_local(|| {
        // Reset advances the generation and clears index state while leaving
        // the allocated texture mirrors in place, like a warmed terminal.
        assert!(atlas.reset().is_ok());
        divan::black_box(atlas.prepare(
            puzzle_slug::ShapeKey(1),
            &path,
            puzzle_slug::atlas::FillRule::NonZero,
            0,
        ))
    });
}

/// Measures interpreter hinting at 12 ppem plus Slug preparation.
/// The unhinted benchmark above starts from a path; this one includes
/// outline extraction because hinting runs during that extraction.
#[divan::bench]
fn jetbrains_hinted_capital_b_12px(bencher: divan::Bencher) {
    // Twelve physical pixels represent the 1× Windows size range that
    // motivated D-27; hinting is inherently ppem-specific.
    let bytes = include_bytes!("../../../assets/fonts/JetBrainsMono-Variable.ttf");
    let font = match puzzle_text::probe::ProbeFont::new(bytes) {
        Ok(font) => font,
        Err(error) => panic!("bundled benchmark font failed: {error}"),
    };
    let hinted = match font.hinted(12.0, puzzle_text::probe::ProbeHintMode::Interpreter) {
        Ok(hinted) => hinted,
        Err(error) => panic!("bundled benchmark hinting failed: {error}"),
    };
    let mut atlas = puzzle_slug::SlugAtlas::default();
    bencher.bench_local(|| {
        // A hinted path must be redrawn per glyph; caching the path here
        // would omit the interpreter that the comparison is measuring.
        let (path, _) = match hinted.glyph('B') {
            Ok(glyph) => glyph,
            Err(error) => panic!("bundled benchmark glyph failed: {error}"),
        };
        assert!(atlas.reset().is_ok());
        // Slug still converts and packs the hinted outline unchanged.
        divan::black_box(atlas.prepare(
            puzzle_slug::ShapeKey(1),
            &path,
            puzzle_slug::atlas::FillRule::NonZero,
            0,
        ))
    });
}

/// Measures two-layer COLR conversion/packing from previously extracted paths.
#[divan::bench]
fn colr_two_layer_fixture(bencher: divan::Bencher) {
    // Disk/font traversal is excluded, matching the §15 outline-to-texels budget.
    // This approved fixture uses a solid backdrop and a radial foreground fill.
    let bytes = include_bytes!("../../../assets/fonts/test/test_glyphs-glyf_colr_1.ttf");
    let face = puzzle_text::face::FontFace::new(puzzle_text::FontData::new(bytes.to_vec(), 0), 400)
        .unwrap_or_else(|e| panic!("COLR benchmark font: {e}"));
    let layers = face
        .color_layers(156, 0xffffffff)
        .unwrap_or_else(|e| panic!("COLR benchmark traversal: {e}"))
        .unwrap_or_else(|| panic!("fixture is not COLR"))
        .layers;
    let mut atlas = puzzle_slug::SlugAtlas::default();
    bencher.bench_local(|| {
        assert!(atlas.reset().is_ok());
        let mut key = 0;
        for layer in &layers {
            // The outer scopes are genuine clip geometry, not skipped metadata.
            for path in std::iter::once(&layer.path).chain(&layer.clips) {
                key += 1;
                let handle = atlas.prepare(
                    puzzle_slug::ShapeKey(key),
                    path,
                    puzzle_slug::atlas::FillRule::NonZero,
                    0,
                );
                divan::black_box(handle.unwrap_or_else(|e| panic!("COLR packing: {e}")));
            }
            divan::black_box(
                layer
                    .paint
                    .texels()
                    .unwrap_or_else(|e| panic!("COLR paint row: {e}")),
            );
        }
    });
}
