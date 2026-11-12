//! Native preparation benchmark for a synthetic curved glyph (design §15.2).
//!
//! The path is built outside timing. Each iteration clears logical atlas
//! contents while keeping allocated CPU mirrors, then measures kurbo cubic
//! conversion, band selection and texel packing together. The reset cost is
//! included, making the reported preparation bound conservative.

/// Runs divan's benchmark registry without a Rust test harness.
fn main() {
    divan::main();
}

/// Measures outline-to-texels work rather than a cached handle lookup.
#[divan::bench]
fn typical_glyph_preparation(bencher: divan::Bencher) {
    let path = puzzle_slug::synthetic::SyntheticShape::TypicalGlyph.path();
    let mut atlas = puzzle_slug::SlugAtlas::default();
    bencher.bench_local(|| {
        // Retained buffers model a warmed long-lived terminal atlas; the
        // logical reset prevents this iteration from hitting the key cache.
        assert!(atlas.reset().is_ok());
        let handle = atlas.prepare(
            puzzle_slug::ShapeKey(1),
            &path,
            puzzle_slug::atlas::FillRule::NonZero,
            0,
        );
        divan::black_box(handle)
    });
}
