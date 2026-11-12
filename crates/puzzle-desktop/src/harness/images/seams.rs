//! Whole-image comparisons cover every tile edge, not just saturated interiors.
//!
//! Yazi omits c/r and emits natural-size images. Odd source dimensions make the
//! last tiles partial, while a second scaled prototype tests fractional UVs.
//! A matching physical placement is the independent untiled sampling reference.
//! Reserved SGR styles and underline-colour IDs must leave no lines in either.
//!
//! Red and green vary by source coordinate; blue alternates every source texel.
//! Thus repeating a source row, inserting paper or sampling the wrong tile cannot
//! hide in a flat-colour interior. Both references share the same immutable image.
//! The comparison changes only placement topology: many tiles versus one quad.
//! The natural pair tests exact texel addressing; the scaled pair tests filtering.
//! No cell border is skipped, including the partially occupied final row/column.
//! This offscreen scene covers GPU assembly and sampling, not Core Animation.
//! Native bounds/drawable mismatches need the separate live layer observation.
use super::*;
use puzzle_term::text::metrics::CellMetrics;

/// Authored smooth opaque colour changes expose missing or mis-sampled edge pixels.
pub(super) fn scene(session: &mut Session, now: Instant) -> Result<(), String> {
    // Keep the entire odd-height natural source visible even at one device pixel ratio.
    let cell = session.cell_metrics().ok_or("seam metrics absent")?;
    session.resize_grid(20, 20, [20 * cell.width_px + 32, 20 * cell.height_px + 32])?;
    let mut rgba = Vec::new();
    for y in 0..137 {
        for x in 0..73 {
            // All channels differ from paper and the placement-ID decoration colour.
            rgba.extend_from_slice(&[
                80 + x as u8,
                80 + y as u8,
                if (x + y) % 2 == 0 { 230 } else { 40 },
                255,
            ]);
        }
    }
    let bytes = payload::encode_png(&rgba, [73, 137])?;
    // Compressing this authored pattern keeps transport framing out of the seam test.
    // The independent protocol fixtures already cover multi-chunk transmission.
    let encoded = payload::base64(&bytes);
    // PNG makes this bounded fixture one protocol chunk, without special decode paths.
    session.feed(
        format!("\x1b_Ga=T,f=100,s=73,v=137,C=1,U=1,i=1,p=7;{encoded}\x1b\\").as_bytes(),
        now,
    );
    let request = session
        .take_image_decode_request()
        .ok_or("seam decode absent")?;
    session.apply(
        SessionMessage::ImageDecoded(puzzle_embed::diagnostics::decode_image(request)),
        now,
    )?;
    // Source decoding must finish before either placement can reference its pixels.
    // Both sides therefore exercise the same admitted image generation.
    // The left image uses text tiles; the right uses one full-image placement.
    // Natural-size rendering covers several rows at both supported DPRs.
    session.feed(b"\x1b[1;11H\x1b_Ga=p,i=1,p=8,C=1\x1b\\", now);
    placeholders(
        session,
        now,
        1,
        7,
        137_u32.div_ceil(cell.height_px),
        73_u32.div_ceil(cell.width_px),
    );
    // Ten columns separate the references, leaving their letterbox paper disjoint.
    // The seven-column scaled image cannot cover the neighbouring reference.
    // Explicit cell bounds downscale into fractional source coordinates; the
    // untiled reference has the same aspect fit, including partial edge cells.
    session.feed(
        b"\x1b_Ga=p,i=1,p=9,U=1,c=7,r=6,C=1\x1b\\\x1b[11;11H\x1b_Ga=p,i=1,p=10,c=7,r=6,C=1\x1b\\",
        now,
    );
    placeholders(session, now, 11, 9, 6, 7);
    Ok(())
}

/// Distinct decorations on each row expose bars at every placeholder boundary.
fn placeholders(
    session: &mut Session,
    now: Instant,
    first: u32,
    placement: u32,
    rows: u32,
    columns: u32,
) {
    for row in 0..rows {
        // These are protocol table entries zero through seven, not arbitrary accents.
        let mark = [
            '\u{0305}', '\u{030d}', '\u{030e}', '\u{0310}', '\u{0312}', '\u{033d}', '\u{033e}',
            '\u{033f}',
        ][row as usize];
        let style = row % 5 + 1;
        // Only the first cell carries its row mark; later cells inherit columns.
        // Each explicit CUP restarts at column zero of that prototype row.
        let cells = format!(
            "\u{10eeee}{mark}{}",
            "\u{10eeee}".repeat(columns as usize - 1)
        );
        // SGR 58 carries the placement ID, while every other text style is reserved.
        session.feed(
            format!(
                "\x1b[{};1H\x1b[38;5;1;58;5;{placement};4:{style};9;53m{cells}",
                first + row
            )
            .as_bytes(),
            now,
        );
    }
}

/// Every image pixel, including both sides of every cell edge, matches the full quad.
pub(super) fn verify(image: &RgbaImage, metrics: CellMetrics) -> Result<(), String> {
    // Font metrics are already device pixels; applying DPR again would compare
    // different source locations and could miss precisely the row-edge defect.
    let right = 10 * metrics.width_px;
    // Natural-size and aspect-fitted samples include every pixel, not just interiors.
    for (top, width, height) in [
        (0, 73, 137),
        (
            10 * metrics.height_px,
            7 * metrics.width_px,
            6 * metrics.height_px,
        ),
    ] {
        for y in top..top + height {
            for x in 0..width {
                // Both placements start inside the same fixed device-pixel inset.
                // Compare matching local coordinates, without resampling the readback.
                let left = super::verify::rgb(image, 16 + x, 16 + y);
                let reference = super::verify::rgb(image, 16 + right + x, 16 + y);
                // No boundary is excluded. A paper gap or decoration fails immediately.
                // Device-pixel sampling makes translated full-placement transforms
                // identical even under nonintegral scaling. Require exact bytes.
                if left != reference {
                    return Err(format!(
                        "placeholder seam at {x},{y}: {left:?} != {reference:?}"
                    ));
                }
            }
        }
    }
    Ok(())
}
