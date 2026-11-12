//! Real JetBrains Mono coverage sheet from the CPU Slug oracle (design §9.6).
//!
//! The sheet uses one prepared outline per character at every size and DPR.
//! This deliberately tests Slug's size independence before M3's GPU port.
//!
//! The input font is parsed only for this command. Its bytes remain alive
//! while `ProbeFont` borrows them; each Unicode scalar is mapped to one
//! glyph outline and em-space advance. M4 later adds shaping, ligatures,
//! fallback and style selection. Here the question is narrower: does the
//! CPU port render real unhinted outlines cleanly at terminal sizes?
//!
//! `SlugAtlas` owns size-independent curves and band lists. Rendering a row
//! changes only the em-to-device scale and integer glyph origins. This
//! catches curves that look acceptable at 20 px but fail at 10 or 11 px,
//! and makes 1× and 2× differences visible on one page. The two palettes
//! beside each other expose light- and dark-background edge behavior.
//!
//! The default text is fixed by design §17.1 to compare with later GPU and
//! font-system work. Ad-hoc options let a reviewer inspect an individual
//! code point or another permitted font without changing the fixture.
//! Output is always a local `target/` artifact, not a shipped font pack.
//!
//! This tool returns a failure for malformed font bytes, absent glyphs,
//! excessive image dimensions and a sheet with no actual oracle ink. The
//! acceptance command renders a fresh file, so a stale PNG cannot pass M2.
/// Pixel placement and coverage accumulation for each text row.
mod render;

use puzzle_slug::{
    ShapeKey, SlugAtlas,
    atlas::{FillRule, ShapeHandle},
    bands::BandBuilder,
    shape::{DEFAULT_TOLERANCE, QuadShape},
};
use puzzle_text::probe::ProbeFont;
use std::{collections::BTreeMap, fs, path::Path};

/// The exact early-review string specified for M2 (§17.1).
/// Its mixed strokes and punctuation expose stem and corner errors.
const REVIEW_TEXT: &str = "Hamburgefonstiv 0Oo1lI {}[] -> => != ==";
/// Review sizes include the thin-stem range at one device pixel per CSS pixel.
/// Consecutive 10–14 px rows matter more than large-size smoothness here.
const REVIEW_SIZES: &[u32] = &[10, 11, 12, 13, 14, 16, 20];
/// One and two physical pixels per CSS pixel expose DPR-specific artifacts.
/// Both sets reuse the same atlas entry for each character.
const REVIEW_DPRS: &[u32] = &[1, 2];

/// A prepared outline and the natural monospace advance, in ems.
/// A space has an advance but no texture handle.
type Prepared = (Option<ShapeHandle>, f32);

/// Builds the standard specimen at the stable review path.
/// This no-argument form is used by acceptance and visual review.
pub(super) fn default_sheet() -> Result<(), String> {
    sheet(
        "assets/fonts/JetBrainsMono-Variable.ttf",
        REVIEW_TEXT,
        REVIEW_SIZES,
        REVIEW_DPRS,
        Path::new("target/slug/specimen.png"),
    )
}

/// Parses the documented font/text/size/DPR options for an ad-hoc sheet.
/// Option values are explicit pairs, keeping the CLI deterministic.
pub(super) fn from_args(args: &[String]) -> Result<(), String> {
    let mut font = None;
    let mut text = REVIEW_TEXT;
    let mut sizes = REVIEW_SIZES.to_vec();
    let mut dprs = REVIEW_DPRS.to_vec();
    let mut single_char = false;
    let mut index = 0;
    while index + 1 < args.len() {
        // A single-glyph request shares the PNG path but additionally
        // prints its band memberships for geometric diagnosis.
        match args[index].as_str() {
            "--font" => font = Some(args[index + 1].as_str()),
            "--text" => text = &args[index + 1],
            "--sizes" => sizes = numbers(&args[index + 1])?,
            "--dpr" => dprs = numbers(&args[index + 1])?,
            "--char" => {
                text = &args[index + 1];
                single_char = true;
            }
            _ => return Err(format!("unknown slug-dump option: {}", args[index])),
        }
        index += 2;
    }
    if index != args.len() {
        // An unmatched final flag is an error, not an implicit default.
        return Err("slug-dump options need values".into());
    }
    let font = font.ok_or("slug-dump requires --font PATH")?;
    if single_char {
        print_glyph_bands(font, text)?;
    }
    // A one-glyph diagnostic must not replace the stable reviewer sheet.
    let output = if single_char {
        "target/slug/glyph.png"
    } else {
        "target/slug/specimen.png"
    };
    sheet(font, text, &sizes, &dprs, Path::new(output))
}

/// Prints the real glyph's bands before the common PNG path renders it.
/// It uses the same kurbo conversion tolerance as atlas insertion.
fn print_glyph_bands(font_path: &str, text: &str) -> Result<(), String> {
    let mut chars = text.chars();
    // One Unicode scalar is a diagnostic unit here; grapheme clusters and
    // ligature shaping become production responsibilities in M4.
    let ch = chars.next().ok_or("--char needs one scalar")?;
    if chars.next().is_some() {
        return Err("--char needs one scalar".into());
    }
    let bytes = fs::read(font_path).map_err(|error| error.to_string())?;
    let font = ProbeFont::new(&bytes).map_err(|error| error.to_string())?;
    let (path, _) = font.glyph(ch).map_err(|error| error.to_string())?;
    let shape =
        QuadShape::from_path(&path, DEFAULT_TOLERANCE).map_err(|error| error.to_string())?;
    let bands = BandBuilder
        .build(&shape, 16)
        .map_err(|error| error.to_string())?;
    // Indices make epsilon overlap, flat-edge exclusion and sorting
    // inspectable without a GPU texture viewer.
    println!(
        "glyph={ch:?} curves={} hbands={} vbands={}",
        shape.curves.len(),
        bands.horizontal.len(),
        bands.vertical.len()
    );
    for (axis, lists) in [("H", &bands.horizontal), ("V", &bands.vertical)] {
        for (index, members) in lists.iter().enumerate() {
            println!("{axis}{index}: {members:?}");
        }
    }
    Ok(())
}

/// Accepts bounded comma-separated device settings to avoid huge PNGs.
/// A zero scale is invalid for the oracle's analytic pixel derivative.
fn numbers(input: &str) -> Result<Vec<u32>, String> {
    let values = input
        .split(',')
        .map(|part| part.parse::<u32>().map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    if values.is_empty() || values.len() > 16 || values.iter().any(|v| *v == 0 || *v > 96) {
        // The cap also bounds the amount of local raster memory a CLI
        // invocation can allocate from untrusted option strings.
        return Err("sizes and DPRs need 1..=16 values in 1..=96".into());
    }
    Ok(values)
}

/// Parses glyphs once, then samples them in each requested device geometry.
/// Any failure leaves the previous PNG intact and returns nonzero; M2
/// acceptance does not trust that file unless this fresh run succeeds.
fn sheet(
    font_path: &str,
    text: &str,
    sizes: &[u32],
    dprs: &[u32],
    output: &Path,
) -> Result<(), String> {
    let bytes = fs::read(font_path).map_err(|error| error.to_string())?;
    let font = ProbeFont::new(&bytes).map_err(|error| error.to_string())?;
    // The atlas owns converted geometry, with no borrow from the font file.
    let mut atlas = SlugAtlas::default();
    let glyphs = prepare_text(&font, &mut atlas, text)?;
    let mut image = render::blank_sheet(text, &glyphs, sizes, dprs)?;
    let ink = render::draw_rows(&atlas, &glyphs, text, sizes, dprs, &mut image)?;
    // The acceptance criterion checks actual oracle ink before writing, so
    // an old file cannot make an empty current render appear successful.
    if ink == 0 {
        return Err("CPU specimen has no ink".into());
    }
    if let Some(folder) = output.parent() {
        // Diagnostic images stay under target/, outside committed assets.
        fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    }
    super::png::write(output, &image)?;
    println!("oracle={} ink_pixels={ink}", output.display());
    Ok(())
}

/// Maps each scalar only once because em-space glyph data is size-independent.
/// Repeated letters share a handle across rows and across the sentence.
fn prepare_text(
    font: &ProbeFont<'_>,
    atlas: &mut SlugAtlas,
    text: &str,
) -> Result<BTreeMap<char, Prepared>, String> {
    let mut glyphs = BTreeMap::new();
    for ch in text.chars() {
        if glyphs.contains_key(&ch) {
            continue;
        }
        let (path, advance) = font.glyph(ch).map_err(|error| error.to_string())?;
        // Space keeps its advance but has no curves, so it must not spend
        // atlas texels or appear as an accidental replacement glyph.
        let handle = if path.is_empty() {
            None
        } else {
            Some(
                atlas
                    .prepare(ShapeKey(ch as u64), &path, FillRule::NonZero, 0)
                    .map_err(|error| error.to_string())?,
            )
        };
        glyphs.insert(ch, (handle, advance));
    }
    Ok(glyphs)
}
