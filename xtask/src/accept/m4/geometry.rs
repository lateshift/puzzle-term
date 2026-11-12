//! GPU pixel-profile checks for joined and dashed builtin cells (design §8.6).
//!
//! A single white pixel at a join let the earlier scenario pass even when
//! the next cell had a displaced grey stroke. Comparing every pixel of the
//! two boundary profiles catches thickness and position changes, including
//! fractional coverage. The test uses fresh Metal readbacks at both DPRs.
//!
//! Dashed strokes deliberately leave a gap at cell borders. Their test
//! samples the stroke centre across two cells and checks equal ink lengths,
//! comparable gaps and an identical repeating cell pattern. At narrow cell
//! widths a nominal half-gap rounds to zero on one side and one on the
//! other; the combined boundary gap must still match interior gaps.
//!
//! The harness stores opaque RGBA8 screenshots on black. Alpha cannot
//! reveal whether a white line dropped to a grey one, so profiles compare
//! the displayed RGB channels. The ±2 byte allowance comes from the
//! reviewer's explicit join tolerance; no scene-specific relaxation is
//! made for curved strokes. A pair of black profiles is rejected so an
//! accidentally missing line cannot pass by equality alone.
//!
//! The fixtures deliberately contain a true rectangle in the main grid:
//! its three vertical columns align with the top and bottom junctions.
//! The arc specimen gives each rounded corner a straight neighbour, then
//! repeats all six horizontal and all six vertical dash weights/counts.
//! An exact script-line check ties these coordinates to the scenario text.
//! Otherwise editing a specimen could leave this pixel test pointed at
//! an empty region while its code and acceptance label still looked sound.
use crate::png::{self, Image};
use std::{fs, path::Path};

/// Grid fixture text pinned so its checked joins cannot silently move.
///
/// The inner row uses columns 0, 5 and 10 for verticals, exactly below
/// the top tee and above the bottom tee that the profile walk samples.
const GRID_LINE: &str = "render-grid 24 4 \"┌────┬────┐\\n│▓░▒ │⠿⣿  │\\n└────┴────┘\\n\"";
/// Arc and dash fixture text pinned to the pixel coordinates below.
///
/// Rows 0–2 form a closed rounded box; rows 4–5 repeat all six vertical
/// dash forms so their longitudinal tiling can be checked across cells.
const ARC_LINE: &str = "render-grid 16 6 \"╭──╮ ┄┄ ╌╌ ┅┅\\n│  │ ╎╎ ┆┆ ┊┊\\n╰──╯ ┈┈ ┉┉\\n     ╍╍ ┅┅ ┉┉\\n    ╎ ╏ ┆ ┇ ┊ ┋\\n    ╎ ╏ ┆ ┇ ┊ ┋\"";

/// Renders the actual harness before either profile check reads its PNGs.
///
/// Both acceptance lines use the result of this invocation, so a failure
/// to render prevents either line from accepting yesterday's target files.
pub(super) fn render_scenes() -> Result<(), String> {
    // The expected source lines prevent a changed specimen from leaving a
    // mechanically green check on coordinates that no longer show joins.
    let script =
        fs::read_to_string("assets/scenarios/text.pzs").map_err(|error| error.to_string())?;
    if script.matches(GRID_LINE).count() != 2 || script.matches(ARC_LINE).count() != 2 {
        return Err("M4 grid/arc fixture changed without updating edge checks".into());
    }
    // `screenshot` occurs after every grid command. Rendering the whole
    // script also runs its separate expect-pixel and expect-ink assertions.
    crate::commands::quiet(
        "cargo",
        &[
            "run",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/text.pzs",
        ],
    )
}

/// Recovers cell dimensions from the harness's 16-pixel outer margins.
///
/// A PNG with a changed target size cannot be sampled using old cell
/// offsets: its dimensions must divide cleanly by the declared grid.
fn scene(path: &str, cols: u32, rows: u32) -> Result<(Image, u32, u32), String> {
    let image = png::read(Path::new(path))?;
    // Grid rendering adds sixteen physical pixels on every side, even at
    // DPR 2; subtract that frame before computing the device-cell size.
    let width = image
        .width
        .checked_sub(32)
        .ok_or("grid PNG lacks margins")?;
    let height = image
        .height
        .checked_sub(32)
        .ok_or("grid PNG lacks margins")?;
    if width % cols != 0 || height % rows != 0 || width / cols == 0 || height / rows == 0 {
        return Err(format!(
            "{path}: image dimensions do not fit {cols}×{rows} cells"
        ));
    }
    Ok((image, width / cols, height / rows))
}

/// Reads a bounded RGB sample without trusting PNG coordinates blindly.
///
/// The PNG reader already checks its byte count; this additional bound
/// makes a bad fixture coordinate a reported error rather than a panic.
fn rgb(image: &Image, x: u32, y: u32) -> Result<[u8; 3], String> {
    if x >= image.width || y >= image.height {
        return Err(format!("sample ({x},{y}) lies outside scene"));
    }
    // Stored scanlines are tight RGBA8 in top-down order; the fourth
    // channel is opaque and intentionally excluded from stroke intensity.
    let offset = ((y as usize * image.width as usize) + x as usize) * 4;
    let bytes = image
        .pixels
        .get(offset..offset + 3)
        .ok_or("short RGBA image")?;
    Ok([bytes[0], bytes[1], bytes[2]])
}

/// Compares every intensity in paired boundary rows or columns.
///
/// `step` chooses a row or column; the same comparison then catches both
/// a shifted centre and a different antialiasing shoulder at a join.
fn profile(
    image: &Image,
    a: [u32; 2],
    b: [u32; 2],
    step: [u32; 2],
    len: u32,
    label: &str,
) -> Result<(), String> {
    // Mark any nonblack sample. Without this guard two absent arms have
    // identical profiles and would satisfy the numerical tolerance.
    let mut ink = false;
    for index in 0..len {
        // Boundary coordinates refer to the final pixel on one side and
        // the first on the other, never the geometric line between pixels.
        let ax = a[0] + index * step[0];
        let ay = a[1] + index * step[1];
        let bx = b[0] + index * step[0];
        let by = b[1] + index * step[1];
        let left = rgb(image, ax, ay)?;
        let right = rgb(image, bx, by)?;
        ink |= left.iter().chain(right.iter()).any(|value| *value > 16);
        // Compare channels independently so a color error cannot hide in
        // an average luminance that happens to match across the seam.
        for channel in 0..3 {
            if left[channel].abs_diff(right[channel]) > 2 {
                return Err(format!(
                    "{label}: ({ax},{ay}) {:?} differs from ({bx},{by}) {:?}",
                    left, right
                ));
            }
        }
    }
    if !ink {
        return Err(format!("{label}: matched blank profiles"));
    }
    Ok(())
}

/// Checks all solid horizontal and vertical join edges in one grid.
///
/// Dashed cells are excluded here because an intentional border gap is
/// checked by `dash_pair`, while the box rectangle must be continuous.
fn joins_in(image: &Image, cw: u32, ch: u32, arc: bool, path: &str) -> Result<(), String> {
    // The main specimen has ten connected top/bottom edges and three
    // vertical columns; the rounded specimen has three and two.
    let horizontal = if arc { 3 } else { 10 };
    let vertical: &[u32] = if arc { &[0, 3] } else { &[0, 5, 10] };
    // The top and bottom row each connect every adjacent box glyph.
    // Scanning the full cell height also detects a stray curved stub.
    for row in [0, 2] {
        for edge in 0..horizontal {
            let x = 16 + (edge + 1) * cw;
            let y = 16 + row * ch;
            profile(
                image,
                [x - 1, y],
                [x, y],
                [0, 1],
                ch,
                &format!("{path}: row {row} joined columns {edge}/{}", edge + 1),
            )?;
        }
    }
    // Only rows 0/1 and 1/2 share vertical arms. The lower dash rows
    // carry no solid box segments and must not enter this join inventory.
    for row in 0..2 {
        for &col in vertical {
            let x = 16 + col * cw;
            let y = 16 + (row + 1) * ch;
            profile(
                image,
                [x, y - 1],
                [x, y],
                [1, 0],
                cw,
                &format!("{path}: column {col} joined rows {row}/{}", row + 1),
            )?;
        }
    }
    Ok(())
}

/// Requires matching GPU boundary profiles in both grid and arc scenes.
///
/// Checking 1× and 2× separately matters: integer stroke bands and Slug
/// coverage can differ after cell width, height and thickness double.
pub(super) fn strict_joins() -> Result<(), String> {
    for (path, cols, rows, arc) in [
        ("target/harness/text/006-grid-1x.png", 24, 4, false),
        ("target/harness/text/008-grid-2x.png", 24, 4, false),
        ("target/harness/text/009-arc-dash-2x.png", 16, 6, true),
        ("target/harness/text/010-arc-dash-1x.png", 16, 6, true),
    ] {
        // The filename is also included in failures so the reviewer can
        // open the exact image and locate the offending joined edge.
        let (image, cw, ch) = scene(path, cols, rows)?;
        joins_in(&image, cw, ch, arc, path)?;
    }
    Ok(())
}

/// Locates each consecutive run of ink in a centre-line pixel sample.
///
/// Half-open spans make dash length `end - start` and adjacent gap
/// `next.start - previous.end`, matching the device-pixel rectangle API.
fn ink_spans(samples: &[bool]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = None;
    for (index, &ink) in samples.iter().enumerate() {
        // A new run begins only after a background sample; touching
        // dashes remain one run and therefore fail the required count.
        if ink && start.is_none() {
            start = Some(index);
        }
        if !ink && let Some(begin) = start.take() {
            spans.push((begin, index));
        }
    }
    // Keep a final dash that reaches the cell boundary. Narrow even
    // cells can quantise one nominal half-gap to zero device pixels.
    if let Some(begin) = start {
        spans.push((begin, samples.len()));
    }
    spans
}

/// Verifies one pair has the named count and uniform integer lengths.
///
/// The pair runs along the stroke direction: horizontal cells sit next
/// to one another, while vertical cells occupy consecutive grid rows.
fn dash_pair(
    image: &Image,
    cw: u32,
    ch: u32,
    row: u32,
    col: u32,
    vertical: bool,
    count: usize,
) -> Result<(), String> {
    // Split at the exact cell edge, not the nearest visible dash edge;
    // a shifted pattern would otherwise appear uniform by itself.
    let half = if vertical { ch } else { cw } as usize;
    let mut samples = Vec::with_capacity(2 * half);
    for offset in 0..2 * half {
        let x = 16 + col * cw + if vertical { cw / 2 } else { offset as u32 };
        let y = 16 + row * ch + if vertical { offset as u32 } else { ch / 2 };
        // The stroke centre is fully covered; a 50% threshold discards
        // antialiasing at a curve edge without changing integer dash runs.
        samples.push(rgb(image, x, y)?.iter().any(|value| *value > 127));
    }
    let label = format!("dash row {row} column {col} vertical={vertical}");
    // Equal cells must have the same quantised sequence. This catches
    // phase drift even when each cell's individual dashes look regular.
    if samples[..half] != samples[half..] {
        return Err(format!("{label}: neighbouring cell patterns differ"));
    }
    // Unicode names specify two, three or four pieces per cell. A merged
    // edge changes the combined count even if both halves contain ink.
    let single = ink_spans(&samples[..half]);
    let paired = ink_spans(&samples);
    if single.len() != count || paired.len() != 2 * count {
        return Err(format!(
            "{label}: expected {count} dashes per cell, saw {} and {}",
            single.len(),
            paired.len()
        ));
    }
    // The gap list includes the cell-border interval; it therefore
    // compares that half-gap pair with every internal gap directly.
    let dash_lengths: Vec<_> = paired.iter().map(|(start, end)| end - start).collect();
    let gaps: Vec<_> = paired
        .windows(2)
        .map(|pair| pair[1].0 - pair[0].1)
        .collect();
    if gaps.contains(&0) || !within_one(&dash_lengths) || !within_one(&gaps) {
        return Err(format!(
            "{label}: uneven dash lengths {dash_lengths:?} or gaps {gaps:?}"
        ));
    }
    Ok(())
}

/// Ensures integer segment lengths vary only by rounding one spare pixel.
///
/// A stronger exact-equality demand is impossible when the cell length
/// is not divisible by the named number of dash periods.
fn within_one(lengths: &[usize]) -> bool {
    let Some(&minimum) = lengths.iter().min() else {
        return false;
    };
    let Some(&maximum) = lengths.iter().max() else {
        return false;
    };
    maximum - minimum <= 1
}

/// Checks every dashed family at 1× and 2× over repeated cells.
///
/// The matrix includes light and heavy forms because thickness changes
/// the perpendicular band but must leave the longitudinal rhythm intact.
pub(super) fn dash_uniformity() -> Result<(), String> {
    for path in [
        "target/harness/text/009-arc-dash-2x.png",
        "target/harness/text/010-arc-dash-1x.png",
    ] {
        // Both screenshots carry the same Unicode matrix at different
        // integer cell sizes, covering the Windows-like 1× path as well.
        let (image, cw, ch) = scene(path, 16, 6)?;
        // Horizontal pairs include light and heavy two-, three- and
        // four-dash forms; row 4/5 repeats every vertical counterpart.
        for (row, col, count) in [
            (0, 5, 3),
            (0, 8, 2),
            (0, 11, 3),
            (2, 5, 4),
            (2, 8, 4),
            (3, 5, 2),
            (3, 8, 3),
            (3, 11, 4),
        ] {
            dash_pair(&image, cw, ch, row, col, false, count)?;
        }
        for (col, count) in [(4, 2), (6, 2), (8, 3), (10, 3), (12, 4), (14, 4)] {
            dash_pair(&image, cw, ch, 4, col, true, count)?;
        }
    }
    Ok(())
}
