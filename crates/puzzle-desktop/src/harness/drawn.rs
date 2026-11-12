//! Drawn-cell assertions separate VT contents from frame cache state (§13.4).
//!
//! A cold frame is built from the same Terminal using fresh fonts, Slug
//! atlas and GPU pipeline state. Comparing exact RGBA bytes catches stale
//! row geometry even when the VT grid itself is correct. The saved images
//! and list of affected cells make that failure inspectable without
//! dumping a large terminal state into an opaque snapshot.
//!
//! The skipped-damage hook is harness-only. It restores one row's prior
//! revision after a feed, so the persistent builder incorrectly reuses
//! that row. A fresh builder ignores the stale revision because its cache
//! is empty. That is a direct test of the detector, not an application
//! path: neither Session nor puzzle-vt contains the hook.
//!
//! There are two deliberately independent observations. `expect-drawn-grid`
//! tries to name cell ink from attachment pixels, while `expect-cold-match`
//! compares the complete incremental attachment with one assembled from a
//! fresh frame builder. A parser bug can change both images identically;
//! a cache bug can leave the incremental image stale while the grid and cold
//! image are correct. Keeping both assertions makes that distinction clear.
//!
//! The fault hook simulates one lost row-revision notification after the
//! parser has already modified cell storage. It exercises the detector's
//! failure path without adding a dangerous switch to production Session.
//! Its scenario has a known old and new word so a reported wrong column can
//! be checked against exact expected evidence, not merely a nonzero exit.
//!
//! A cursor is presentation geometry, not stored text. A filled cursor can
//! cover a blank, and an inverted cursor can reverse glyph colors. The
//! recognizer may use a contextual template there, but a pure text-to-grid
//! assertion excludes the cursor cell because it has no unique visible
//! character until the overlay is removed. The cold-image comparison still
//! checks those cursor pixels exactly.

use super::Scenario;
use crate::{harness::text, png};
use puzzle_render::{FrameRenderer, RgbaImage};
use puzzle_term::{frame::RenderFrame, frame_builder::FrameBuilder, text::font_set::FontSet};
use std::collections::BTreeSet;

/// Rasterized character templates and their coverage-based matcher.
pub(super) mod recognize;

/// Prefix checks include a cold render when an inferred row is wrong.
mod prefix;

/// Owns K3's command names so the general harness dispatcher stays small.
///
/// Returning `None` delegates non-K3 lines to the older scenario parser.
/// A recognized spelling always returns a result, including a parse error;
/// otherwise a malformed drawn assertion could be mistaken for an unrelated
/// command and fail with a less useful message.
pub(super) fn command(state: &mut Scenario, line: &str) -> Option<Result<(), String>> {
    let mut parts = line.split_whitespace();
    match parts.next()? {
        "dump-drawn" => Some(dump(state, parts.next())),
        "expect-drawn" => Some(prefix::expect_prefix(state, line)),
        "expect-drawn-not" => Some(expect_absent(state, line)),
        "expect-drawn-grid" => Some(expect_grid(state)),
        "expect-drawn-row-changed" => Some(recognize::expect_row_changed(state, parts.next())),
        "expect-drawn-any-row-changed" => Some(recognize::expect_any_row_changed(state)),
        "expect-cold-match" => Some(cold_match(state)),
        "test-skip-row-damage" => Some(skip_damage(state, parts.next())),
        _ => None,
    }
}

/// Writes the text inferred from pixels, with uncertain cells annotated.
///
/// The notes file is written even if recognition is certain everywhere.
/// That gives review scripts a stable companion path and keeps uncertainty
/// out of the text file consumed by `expect-drawn`.
pub(super) fn dump(state: &mut Scenario, raw: Option<&str>) -> Result<(), String> {
    let label = raw.ok_or("dump-drawn needs a label")?;
    let result = recognize::cached(state)?;
    let (drawn, annotations) = (result.text(), result.notes.join("\n"));
    // Number both outputs in command order. A later render can produce a
    // different image, so an unnumbered overwrite would lose the chronology.
    let path = state.path(label, "drawn.txt");
    std::fs::write(&path, drawn).map_err(|error| error.to_string())?;
    let notes = state.path(label, "drawn-notes.txt");
    std::fs::write(notes, annotations).map_err(|error| error.to_string())
}

/// Rejects text appearing in a physical cell rectangle of drawn output.
///
/// Nvim's show-command flash appears only in a small bottom-row region.
/// Restricting the search prevents unrelated text elsewhere on the screen
/// from hiding a regression or making the check nondeterministic.
pub(super) fn expect_absent(state: &mut Scenario, line: &str) -> Result<(), String> {
    let region = line
        .split_whitespace()
        .nth(1)
        .ok_or("missing drawn range")?;
    let (first, last) = region.split_once('-').ok_or("drawn range needs -")?;
    let start = parse_cell(first)?;
    let end = parse_cell(last)?;
    // Region coordinates are row:column, matching the terminal and the
    // `expect-ink` convention. They are inclusive at both ends.
    let wanted = super::preview::quoted(line, "expect-drawn-not")?;
    let result = recognize::cached(state)?;
    for row in start.0..=end.0 {
        let cells = result.rows.get(row).ok_or("drawn row outside grid")?;
        let selected: String = cells
            .chars()
            .skip(start.1)
            .take(end.1 - start.1 + 1)
            .collect();
        // One failed row is enough, but the error names its source range
        // so the reviewer can inspect the corresponding PNG crop.
        if selected.contains(wanted) {
            return Err(format!(
                "drawn range {region} contains {wanted:?} at row {row}"
            ));
        }
    }
    Ok(())
}

/// Parses the row:column coordinate convention shared by ink assertions.
///
/// Keeping row and column in a tuple avoids the older pixel x:y convention
/// that once made `expect-ink` pass on arbitrary parts of a screenshot.
fn parse_cell(raw: &str) -> Result<(usize, usize), String> {
    let (row, column) = raw.split_once(':').ok_or("drawn cell needs row:column")?;
    Ok((
        row.parse().map_err(|_| "invalid row")?,
        column.parse().map_err(|_| "invalid column")?,
    ))
}

/// Reports every physical cell whose recognized glyph differs from VT text.
///
/// This is intentionally stricter than a screenshot-wide ink count. A stale
/// word can have the same number of dark pixels as its replacement. The
/// output lists each physical mismatch so it can be compared with the cold
/// diff and with the grid dump from the same scenario step.
pub(super) fn expect_grid(state: &mut Scenario) -> Result<(), String> {
    let _ = recognize::cached(state)?;
    let result = state.drawn_cache.as_ref().ok_or("drawn cache missing")?;
    let term = state.terminal.as_ref().ok_or("drawn grid needs terminal")?;
    let mut errors = Vec::new();
    for (row_index, drawn) in result.rows.iter().enumerate() {
        let row = term.screen().row(row_index).ok_or("grid row missing")?;
        for (column, cell) in row.cells.iter().enumerate() {
            // The cursor is an overlay drawn after cell ink. Its block can
            // hide a blank or invert a glyph, so it is not a text mismatch.
            if term.modes.get(true, 25) == Some(true)
                && row_index == term.cursor.row
                && column == term.cursor.column
            {
                continue;
            }
            if cell.flags.contains(puzzle_term::vt::CellFlags::WIDE_SPACER) {
                // A spacer is a continuation of the previous glyph, not a
                // second character that the recognizer should identify.
                continue;
            }
            let expected = char::from_u32(cell.ch).unwrap_or(' ');
            let actual = drawn.chars().nth(column).unwrap_or(' ');
            // A zero scalar represents an unwritten blank. Treating it as
            // U+0000 would create a mismatch despite a blank attachment.
            if (if cell.ch == 0 { ' ' } else { expected }) != actual {
                errors.push(format!(
                    "{row_index}:{column} grid={expected:?} drawn={actual:?}"
                ));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        // All mismatches are retained; returning after the first would hide
        // a broad stale row behind one coincidental glyph substitution.
        Err(errors.join(", "))
    }
}

/// Saves visible revisions before a harness feed mutates VT state.
///
/// Row versions are the production cache key's damage component. Saving
/// just this compact vector suffices to corrupt one key later, without
/// cloning the grid or introducing a shadow parser in the harness.
pub(super) fn remember_versions(state: &mut Scenario) -> Result<(), String> {
    let term = state
        .terminal
        .as_ref()
        .ok_or("damage hook needs terminal")?;
    state.old_row_versions = (0..term.size().1)
        .map(|index| term.screen().row(index).map(|row| row.version).unwrap_or(0))
        .collect();
    // A feed may scroll and change row identities. The subsequent hook is
    // only used in a controlled scenario whose row identity stays stable.
    Ok(())
}

/// Simulates one dropped row-damage notice in the harness only.
///
/// If the feed did not advance the row revision, the hook refuses to run.
/// A green detector test must have actually created stale cache metadata;
/// otherwise a no-op injection could make the failure assertion meaningless.
pub(super) fn skip_damage(state: &mut Scenario, raw: Option<&str>) -> Result<(), String> {
    let index: usize = raw
        .ok_or("test-skip-row-damage needs a row")?
        .parse()
        .map_err(|_| "invalid damage row")?;
    let prior = *state
        .old_row_versions
        .get(index)
        .ok_or("no saved row revision")?;
    // The row's cells remain newly written. Only its revision is restored,
    // matching the failure mode where a damage journal misses a changed row.
    let row = state
        .terminal
        .as_mut()
        .ok_or("damage hook needs terminal")?
        .screen_mut()
        .row_mut(index)
        .ok_or("damage row outside grid")?;
    if row.version == prior {
        return Err("damage row did not change before injection".into());
    }
    // A cache lookup now sees the old revision even though its cells have
    // changed. The cold frame will read current cells and expose the stale
    // persistent image as a real pixel disagreement.
    row.version = prior;
    Ok(())
}

/// Requires persistent and fresh frame pipelines to draw equal pixels.
///
/// Comparing complete RGBA arrays includes backgrounds, decorations,
/// cursors and the outer padding. The comparison is byte exact because
/// both images use the same native device and pixel format in this run.
pub(super) fn cold_match(state: &mut Scenario) -> Result<(), String> {
    let incremental = state
        .image
        .as_ref()
        .ok_or("expect-cold-match needs a rendered frame")?;
    let cold = cold_image(state)?;
    // A successful equality is evidence that no cached geometry or atlas
    // state was visibly stale at this snapshot, not a claim about parser truth.
    if incremental.pixels == cold.pixels {
        return Ok(());
    }
    write_difference(state, &cold)
}

/// Builds the contextual reference with fresh row caches and GPU stores.
///
/// The cold builder receives the same font bytes, size, DPR, surface extent
/// and blink phase as the persistent builder. The only intentional change
/// is cache history. If these presentation inputs differed, this check
/// would report a false stale-row bug rather than isolate cache behavior.
pub(super) fn cold_image(state: &Scenario) -> Result<RgbaImage, String> {
    let incremental = state
        .image
        .as_ref()
        .ok_or("cold image needs rendered frame")?;
    let term = state
        .terminal
        .as_ref()
        .ok_or("expect-cold-match needs terminal")?;
    let fonts = FontSet::new(
        text::read_font("assets/fonts/JetBrainsMono-Variable.ttf")?,
        Some(text::read_font(
            "assets/fonts/JetBrainsMono-Italic-Variable.ttf",
        )?),
        Some(text::read_font(
            "assets/fonts/SymbolsNerdFontMono-Regular.ttf",
        )?),
    )?;
    let mut context =
        puzzle_term::frame_builder::context::TextContext::new(fonts, 14.0, state.dpr)?;
    let mut builder = FrameBuilder::new(&context);
    if let Some(warm) = state.frame_builder.as_ref() {
        // Cursor and SGR-5 phases are presentation state, not VT cells;
        // compare both builders at the same phase.
        builder.set_blink_phase(warm.blink_phase());
    }
    // Cache history is the sole intentional difference: explicit preferences
    // must also reach this independent builder rather than silently using defaults.
    builder.set_cursor_settings(state.cursor_settings);
    if let Some(selection) = &state.theme_selection {
        builder.set_theme(&selection.active);
    }
    builder.set_surface_size([incremental.width, incremental.height]);
    // A cold builder starts with no prepared row handles; building the
    // entire visible screen forces text and GPU instance preparation afresh.
    let mut frame = RenderFrame::default();
    builder.build(&mut context, term, &mut frame)?;
    let mut renderer = FrameRenderer::new(&state.renderer, wgpu::TextureFormat::Rgba8Unorm);
    let (cold, _) = renderer
        .render_to_image(&state.renderer, &mut frame)
        .map_err(|error| error.to_string())?;
    Ok(cold)
}

/// Saves both images and a visible diff before reporting affected cells.
///
/// Magenta marks any unequal RGBA pixel, while black remains unchanged.
/// The PNG trio lets a reviewer correlate the machine-reported cell list
/// with the actual old and new images without trusting the recognizer.
fn write_difference(state: &mut Scenario, cold: &RgbaImage) -> Result<(), String> {
    // Failure evidence owns the prior pixels so numbered path creation can
    // advance the scenario while the comparison image is still available.
    let previous = state.image.as_ref().ok_or("incremental image lost")?;
    let incremental = RgbaImage {
        width: previous.width,
        height: previous.height,
        pixels: previous.pixels.clone(),
    };
    let metrics = state
        .frame_builder
        .as_ref()
        .ok_or("cell metrics missing")?
        .metrics();
    let (diff, cells) = difference_mask(&incremental, cold, metrics.width_px, metrics.height_px)?;
    let old_path = state.path("incremental", "png");
    // Write all evidence before returning an error; acceptance inspects
    // these paths to prove the failure was a pixel disagreement.
    png::write(&old_path, &incremental)?;
    let cold_path = state.path("cold", "png");
    png::write(&cold_path, cold)?;
    let diff_path = state.path("difference", "png");
    png::write(&diff_path, &diff)?;
    let locations = cells
        .iter()
        .map(|(row, column)| format!("{row}:{column}"))
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "cold frame differs at cells {locations}; evidence: {}, {}, {}",
        old_path.display(),
        cold_path.display(),
        diff_path.display()
    ))
}

/// Marks unequal attachment pixels and projects them onto device cells.
fn difference_mask(
    incremental: &RgbaImage,
    cold: &RgbaImage,
    width: u32,
    height: u32,
) -> Result<(RgbaImage, BTreeSet<(u32, u32)>), String> {
    // The diff's cell coordinates use the same sixteen-pixel border and
    // integer device-cell metrics as the frame builder and recognizer.
    let mut diff = RgbaImage {
        width: cold.width,
        height: cold.height,
        pixels: vec![0; cold.pixels.len()],
    };
    let mut cells = BTreeSet::new();
    for (index, (old, new)) in incremental
        .pixels
        .chunks_exact(4)
        .zip(cold.pixels.chunks_exact(4))
        .enumerate()
    {
        if old != new {
            // `chunks_exact(4)` counts pixels, not channels. Dividing by
            // image width restores the physical x/y readback coordinate.
            let x = u32::try_from(index).map_err(|_| "image index overflow")? % cold.width;
            let y = u32::try_from(index).map_err(|_| "image index overflow")? / cold.width;
            if x >= 16 && y >= 16 {
                // The border is not a grid cell. Differences there remain
                // visible in the PNG even though they have no row:col name.
                cells.insert(((y - 16) / height, (x - 16) / width));
            }
            let at = index * 4;
            diff.pixels[at..at + 4].copy_from_slice(&[255, 0, 255, 255]);
        }
    }
    Ok((diff, cells))
}

#[cfg(test)]
/// Actual status pixels and failure retention protect the diagnostic witness.
mod tests;
