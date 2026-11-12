//! Native measurements for M4's shaping and startup paths (design §15, P7).
//!
//! A changed row invokes complete layout: metrics, grapheme collection,
//! fallback lookup, a run-cache hit and placement into reused storage.
//! The user approved separate limits for that full path and three shaping
//! subpaths; `shaping` measures each without font-loading I/O. A prebuilt
//! pack needs bounded decoding before first upload. D-27's light hinter
//! prepares printable ASCII once per ppem after the font arrives.
//!
//! The shipped pack is read before timing, so this measures parsing rather
//! than disk cache. Font bytes and face construction are also outside the
//! timed shaping and hinting samples. The command reports each median and
//! exits nonzero on the limits; divan remains useful for detailed profiles.
//!
//! The 240-cell input includes ordinary letters and coding ligatures.
//! The warmed run cache skips Harfrust but layout still resolves all 240
//! clusters and reuses placement vectors. Separate divan benchmarks
//! isolate direct ASCII, run-cache and uncached Harfrust costs. The pack
//! check has no TTF access:
//! its metrics and cmap are enough for a first line. The separate native
//! pack probe covers actual texture upload and visible GPU ink.
//! Hinted samples use a fresh Slug atlas each time; otherwise repeated
//! samples would measure a handle lookup rather than outline preparation.
use puzzle_text::{
    FontData,
    font_set::{FontSet, FontStyle},
    glyph::{GlyphBackend, slug::SlugBackend},
    hinting::HintingSetting,
    layout::{self, LayoutOptions},
    pack::FontPack,
    shape::TextShaper,
};
use std::{fs, time::Instant};
mod shaping;

/// Loads the approved face through caller-owned bytes.
fn fonts() -> Result<FontSet, String> {
    let bytes = fs::read("assets/fonts/JetBrainsMono-Variable.ttf")
        .map_err(|error| format!("benchmark font: {error}"))?;
    FontSet::new(FontData::new(bytes, 0), None, None)
}

/// Holds the default 1× grid and D-27 light-hinting selection fixed.
fn options(ligatures: bool) -> LayoutOptions<'static> {
    LayoutOptions {
        size: 12.0,
        dpr: 1.0,
        origin_px: [0, 0],
        style: FontStyle::Regular,
        hinting: HintingSetting::Auto,
        ligatures,
        features: &[],
        cursor_byte: None,
        transformed: false,
        symbol_overflow: true,
        builtin_glyphs: true,
        color: 0xffff_ffff,
    }
}

/// Returns a sorted sample's median in microseconds.
fn median(samples: &mut [f64]) -> Result<f64, String> {
    // Empty or nonfinite timings would make a numerical pass meaningless.
    if samples.is_empty() || samples.iter().any(|value| !value.is_finite()) {
        return Err("benchmark produced no finite samples".into());
    }
    samples.sort_by(f64::total_cmp);
    Ok(samples[samples.len() / 2])
}

/// Measures validated pack decoding and a first ASCII line without a TTF.
fn pack_start(bytes: &[u8]) -> Result<f64, String> {
    // Decode anew for every sample so a retained BTreeMap or texture plane
    // cannot conceal the work required on page startup.
    let mut samples = Vec::with_capacity(100);
    for _ in 0..100 {
        let start = Instant::now();
        let pack = FontPack::decode(bytes)?;
        let scene = puzzle_term_pack_scene(pack)?;
        std::hint::black_box(scene);
        samples.push(start.elapsed().as_secs_f64() * 1e6);
    }
    median(&mut samples)
}

/// Uses the pack directory and metrics without depending on puzzle-term.
fn puzzle_term_pack_scene(pack: FontPack) -> Result<(u32, usize), String> {
    // xtask may depend on puzzle-text but not puzzle-term (§4.1). This
    // small measurement checks the same cmap and sizing operations while
    // the native pack probe proves actual texture upload and draw.
    let cell = (pack.metrics.zero_advance * 14.0).round().max(1.0) as u32;
    // Include the space: it has no ink, yet its cmap record preserves the
    // following glyph's fixed-cell origin when the font is not loaded.
    let glyphs = "PACK B"
        .chars()
        .filter(|ch| pack.glyphs.contains_key(&(*ch as u32)))
        .count();
    if glyphs != 6 {
        return Err("pack misses first-line ASCII".into());
    }
    Ok((cell, glyphs))
}

/// Prepares the 95 printable ASCII scalars through the real hinted backend.
fn hinted_ascii(fonts: &mut FontSet, shaper: &mut TextShaper) -> Result<f64, String> {
    // The complete printable set includes every glyph the terminal might
    // need immediately for a prompt, not only the capital B microbench.
    let ascii: String = (0x20_u8..=0x7e).map(char::from).collect();
    let mut samples = Vec::with_capacity(7);
    for _ in 0..7 {
        // A new backend forces every ppem-specific Slug shape to be packed
        // again, while skrifa's face/hinter stays loaded between frames.
        let mut backend = SlugBackend::default();
        let start = Instant::now();
        let placed = layout::line(fonts, shaper, &ascii, options(false))?;
        for glyph in &placed.glyphs {
            std::hint::black_box(backend.prepare(glyph, fonts)?);
        }
        samples.push(start.elapsed().as_secs_f64() * 1e6);
    }
    median(&mut samples)
}

/// Measures layout and enforces only approved first-frame CPU budgets.
pub(crate) fn run() -> Result<(), String> {
    // Asynchronous font fetching is outside this CPU stage; these loaded
    // bytes isolate layout, pack parsing and first-use hint preparation.
    let mut fonts = fonts()?;
    let mut shaper = TextShaper::default();
    let source = "Hamburgefonstiv 0Oo1lI -> => != ==".repeat(8);
    let row = &source[..240];
    let cache = shaping::cache_hit(&fonts, row)?;
    let ascii = shaping::ascii_miss(&fonts, row)?;
    let harfrust = shaping::harfrust_miss(&fonts, row)?;
    let shaping = shaping::shaping_row(&mut fonts, &mut shaper, row)?;
    let bytes = fs::read("assets/fontpacks/jetbrains-regular.pzfp")
        .map_err(|error| format!("benchmark pack: {error}"))?;
    let pack = pack_start(&bytes)?;
    let hinted = hinted_ascii(&mut fonts, &mut shaper)?;
    println!("M4_METRIC cache_hit_us={cache:.6}");
    println!("M4_METRIC ascii_miss_us={ascii:.6}");
    println!("M4_METRIC harfrust_miss_us={harfrust:.6}");
    println!("M4_METRIC row_layout_us={shaping:.6}");
    println!("M4 pack decode and first-line map: {pack:.2} us (budget 80000 us)");
    println!("M4 hinted printable ASCII first preparation: {hinted:.2} us (budget 80000 us)");
    if pack > 80_000.0 || hinted > 80_000.0 {
        return Err("M4 first-frame CPU budget exceeded".into());
    }
    Ok(())
}
