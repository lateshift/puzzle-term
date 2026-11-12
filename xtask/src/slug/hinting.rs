//! CPU-only font hinting comparison for open decision D-27.
//!
//! Skrifa supplies true interpreter and light auto-hinted outlines at each
//! device ppem. They enter the same kurbo conversion, atlas and Slug oracle
//! as M2's unhinted specimen. The rows keep both B and C branches because
//! choosing a winner requires human visual judgment on a 1× display.
//!
//! D rows snap the baseline to an integer pixel (as every row already does)
//! and nudge cap height by no more than 3%. E rows set the reference W flag
//! for sqrt coverage. F1/F2 only change compositing after coverage; they do
//! not modify the shader formula or the CPU oracle's root math (R11).
//! Each named variant gets a full-resolution sheet and a 3× nearest-neighbor
//! crop so the reviewer can inspect individual pixels without interpolation.
/// Builds the sheet from actual oracle samples and enlarges its crops.
mod render;
/// Places all variant rows and saves the comparison evidence.
mod sheet;
/// Keeps variant metadata and the requested sRGB transfer functions together.
mod variant;

use puzzle_text::probe::ProbeFont;
use std::fs;

/// Default text probes upper/lower strokes and terminal punctuation (§17.1).
const DEFAULT_TEXT: &str = "Hamburgefonstiv 0Oo1lI {}[] -> => != ==";
/// The user specified five contiguous sizes on one physical pixel per CSS px.
const DEFAULT_SIZES: &[u32] = &[10, 11, 12, 13, 14];

/// Parses the specimen's font/text/size/DPR options with fixed 1× scope.
pub(super) fn run(args: &[String]) -> Result<(), String> {
    let mut font_path = "assets/fonts/JetBrainsMono-Variable.ttf";
    let mut text = DEFAULT_TEXT;
    let mut sizes = DEFAULT_SIZES.to_vec();
    let mut index = 0;
    while index + 1 < args.len() {
        match args[index].as_str() {
            "--font" => font_path = &args[index + 1],
            "--text" => text = &args[index + 1],
            "--sizes" => sizes = parse_sizes(&args[index + 1])?,
            "--dpr" if args[index + 1] == "1" => {}
            "--dpr" => return Err("D-27 comparison is DPR 1 only".into()),
            _ => return Err(format!("unknown compare-hinting option: {}", args[index])),
        }
        index += 2;
    }
    if index != args.len() || text.is_empty() {
        return Err("comparison options need values and nonempty text".into());
    }
    let bytes = fs::read(font_path).map_err(|error| error.to_string())?;
    let font = ProbeFont::new(&bytes).map_err(|error| error.to_string())?;
    // The sheet owns its render atlases; the font remains borrowed from the
    // bytes above until every hinted outline has been extracted.
    sheet::write(&font, text, &sizes)
}

/// Bounds custom size requests to the approved small-text comparison range.
fn parse_sizes(input: &str) -> Result<Vec<u32>, String> {
    let sizes = input
        .split(',')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if sizes.is_empty() || sizes.len() > 5 || sizes.iter().any(|size| !(10..=14).contains(size)) {
        return Err("D-27 sizes must be a nonempty subset of 10..=14".into());
    }
    Ok(sizes)
}
