//! Diagnoses an incorrect row prefix using both the drawn pixels and a fresh render.
//!
//! The first observation names what the glyph recognizer saw in the warm
//! attachment. A second, cold render tests whether an incremental frame
//! cache was stale; equal pixels leave parser state and recognition to inspect.

use super::{Scenario, cold_match, recognize};

/// Checks a row prefix inferred from the rendered pixels.
///
/// Prefixes let a live application vary its status tail without weakening
/// the asserted text at the left of the row. The source remains the GPU
/// readback, so a correct VT row cannot satisfy a stale drawn prefix.
pub(super) fn expect_prefix(state: &mut Scenario, line: &str) -> Result<(), String> {
    let row: usize = line
        .split_whitespace()
        .nth(1)
        .ok_or("missing drawn row")?
        .parse()
        .map_err(|_| "invalid drawn row")?;
    let wanted = super::super::preview::quoted(line, "expect-drawn")?;
    let result = recognize::cached(state)?;
    let actual = result.rows.get(row).ok_or("drawn row outside grid")?;
    if actual.starts_with(wanted) {
        return Ok(());
    }
    // Preserve the failed observation before a cold comparison mutably
    // borrows the scenario to save evidence if its pixels disagree.
    let actual = actual.clone();
    let notes = result
        .notes
        .iter()
        .filter(|note| note.starts_with(&format!("{row}:")))
        .cloned()
        .collect::<Vec<_>>();
    // A failed prefix alone pays for the full independent render. Equal
    // pixels rule out stale warm geometry; a difference names affected cells.
    let cold = match cold_match(state) {
        Ok(()) => "warm/cold pixels identical".to_owned(),
        Err(error) => format!("warm/cold comparison: {error}"),
    };
    // The VT row is a third observation: identical warm/cold pixels can
    // still reflect a parser or child-output error rather than bad OCR.
    let grid = state
        .terminal
        .as_ref()
        .and_then(|term| {
            super::super::preview::grid_dump(term)
                .lines()
                .nth(row)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "<unavailable>".to_owned());
    Err(format!(
        "drawn row {row} was {actual:?}, wanted {wanted:?}; VT {grid:?}; scores {notes:?}; {cold}"
    ))
}
