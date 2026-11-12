//! Four release measurements for changed terminal rows (design §8.4, §15).
//!
//! Timing includes the work each path actually promises: an exact run
//! cache hit borrows glyph IDs; an ASCII miss fills IDs through the stored
//! 128-entry cmap; a Harfrust miss shapes and inserts a result; full row
//! layout collects graphemes, chooses fallback faces and places neutral
//! glyphs into caller-reused buffers. These are separate because a fast
//! run-cache lookup does not prove a changed row is cheap.
//!
//! The loaded font is shared outside all clocks. The uncached shaping
//! cases create a fresh TextShaper inside the clock so the same 240-byte
//! fixture cannot accidentally become a cache hit. Full row timing warms
//! its cache and buffers before sampling to model P4 changed rows after
//! startup. Each result is a median, making one OS scheduling interruption
//! less likely to trigger a false budget failure.
//!
//! The input includes letters, spaces and `->`/`!=`/`==` pairs. With coding
//! features enabled, those pairs require OpenType substitution; with them
//! disabled, each printable byte maps directly to one glyph ID. Both paths
//! therefore process equal byte and cell counts while testing different
//! work. Harfrust's script plan has already been parsed into the face.
//!
//! These timers run in the release profile used for the native §15 budget.
//! They report time per complete 240-cell call, not per glyph. A failure
//! remains attributable to a named path in `accept M4`; the measurement
//! code never adjusts a limit according to machine load or prior runs.
use super::{median, options};
use puzzle_text::{
    font_set::{FontSet, FontStyle},
    layout::RowLayout,
    shape::{ShapingOptions, TextShaper},
};
use std::time::Instant;

/// Measures complete cached layout through the reused row buffers.
pub(super) fn shaping_row(
    fonts: &mut FontSet,
    shaper: &mut TextShaper,
    row: &str,
) -> Result<f64, String> {
    // The source is ASCII, so this byte slice is exactly 240 grid cells.
    let mut storage = RowLayout::default();
    // Cache and buffer warmup is outside the timed changed-row path.
    storage.line(fonts, shaper, row, options(true))?;
    repeated(
        || {
            let placed = storage.line(fonts, shaper, row, options(true))?;
            std::hint::black_box(placed.glyphs.len());
            Ok(())
        },
        100,
    )
}

/// Times one repeated operation per sample and returns the median per call.
fn repeated(
    mut operation: impl FnMut() -> Result<(), String>,
    iterations: usize,
) -> Result<f64, String> {
    let mut samples = Vec::with_capacity(101);
    for _ in 0..101 {
        let start = Instant::now();
        for _ in 0..iterations {
            operation()?;
        }
        samples.push(start.elapsed().as_secs_f64() * 1e6 / iterations as f64);
    }
    median(&mut samples)
}

/// Isolates the exact-key hit from Unicode layout and fallback costs.
pub(super) fn cache_hit(fonts: &FontSet, row: &str) -> Result<f64, String> {
    // The face is borrowed once so font loading is outside the clock.
    let face_id = fonts.primary_face(FontStyle::Regular);
    let face = fonts.face(face_id).ok_or("benchmark face is missing")?;
    let mut shaper = TextShaper::default();
    let settings = ShapingOptions {
        ligatures: true,
        features: &[],
    };
    shaper.shape(face_id, face, row, settings)?;
    // Repeated lookups preserve the exact text and feature key; the
    // returned slice is borrowed and cannot trigger cache insertion.
    repeated(
        || {
            let glyphs = shaper.shape(face_id, face, row, settings)?;
            std::hint::black_box(glyphs.len());
            Ok(())
        },
        1_000,
    )
}

/// Includes a new direct ASCII result without allowing a cache hit.
pub(super) fn ascii_miss(fonts: &FontSet, row: &str) -> Result<f64, String> {
    let face_id = fonts.primary_face(FontStyle::Regular);
    let face = fonts.face(face_id).ok_or("benchmark face is missing")?;
    let settings = ShapingOptions {
        ligatures: false,
        features: &[],
    };
    if !TextShaper::ascii_fast_path(face, row, settings)? {
        return Err("ASCII benchmark did not use direct cmap".into());
    }
    // A new shaper is essential here: repeated fixture text otherwise
    // turns this into the cache-hit case and conceals direct-map cost.
    repeated(
        || {
            let mut shaper = TextShaper::default();
            let glyphs = shaper.shape(face_id, face, row, settings)?;
            std::hint::black_box(glyphs.len());
            Ok(())
        },
        100,
    )
}

/// Includes fresh Harfrust buffer construction and run-cache insertion.
pub(super) fn harfrust_miss(fonts: &FontSet, row: &str) -> Result<f64, String> {
    let face_id = fonts.primary_face(FontStyle::Regular);
    let face = fonts.face(face_id).ok_or("benchmark face is missing")?;
    let settings = ShapingOptions {
        ligatures: true,
        features: &[],
    };
    if TextShaper::ascii_fast_path(face, row, settings)? {
        return Err("ligature benchmark bypassed Harfrust".into());
    }
    // The face's substitution data is loaded, but every timed call must
    // execute Harfrust and own a new exact-key cache result.
    repeated(
        || {
            let mut shaper = TextShaper::default();
            let glyphs = shaper.shape(face_id, face, row, settings)?;
            std::hint::black_box(glyphs.len());
            Ok(())
        },
        100,
    )
}
