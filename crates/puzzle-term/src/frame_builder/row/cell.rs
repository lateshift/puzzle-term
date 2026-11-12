//! Resolves one VT cell into colored row geometry (§10.5).
//! Selection ink is resolved before ordinary minimum contrast.
//! The cached alternative uses selected paper and the optional foreground.
//! A later selection edge therefore needs no cell walk or shaping.
//! Parallel colors follow all emitted instances, including synthetic bold.
//!
//! The byte parser stores tagged colors and sparse extras; neither is a
//! final GPU color. This step consults terminal OSC overrides and the
//! current color policy while preserving a cell's physical grid column.
//! Backgrounds and SGR decorations include wide spacer cells, whereas
//! shaping must skip those placeholders to avoid painting duplicate ink.
use super::{RowShape, RunStyle, font_style, paint, wave};
use crate::{
    frame_builder::colors::ColorPolicy,
    frame_snapshot::{PaintState, SnapshotCell},
};
use puzzle_text::metrics::CellMetrics;
use puzzle_vt::cell::{Attrs, CellFlags};

/// Paints one physical column, then extends its text run if it owns ink.
///
/// `PLACEHOLDERS` is selected from the row summary before this loop runs.
/// Ordinary rows therefore compile without a cluster-prefix inspection.
/// `column` remains a grid coordinate even for wide spacers and hidden text;
/// decoration geometry must not shift when those cells produce no glyph.
/// Paint uses the sealed snapshot's palette and the builder's current policy.
pub(super) fn paint<const PLACEHOLDERS: bool>(
    shape: &mut RowShape<'_>,
    state: &PaintState,
    column: usize,
    cell: SnapshotCell<'_>,
    metrics: CellMetrics,
    policy: &ColorPolicy,
) -> Result<(), String> {
    // Count visits before suppression: blank or hidden cells still cost paint
    // classification, even when they emit no instance in the final draw list.
    #[cfg(test)]
    crate::frame_builder::testing_work::charge(1, 1);
    // Placeholder text attributes are reserved by kitty; foreground/underline
    // encode identifiers, so inverse must not turn an image ID into cell paper.
    let placeholder = PLACEHOLDERS && {
        #[cfg(test)]
        PLACEHOLDER_CHECKS.with(|count| count.set(count.get() + 1));
        crate::images::placeholders::is_placeholder(cell.text)
    };
    let attrs = if placeholder {
        // The raw SGR flags remain available for ID decoding elsewhere, but
        // image metadata must not participate in ordinary text-color inversion.
        Attrs::empty()
    } else {
        cell.raw.attrs
    };
    // OSC palette overrides can recolor stored cells without editing their
    // row versions; style-epoch invalidation leads those cells back here.
    let (base_ink, _) = policy.resolve_uncontrasted(state, cell.raw.fg, cell.raw.bg, attrs);
    let selected_ink = policy.selection_ink(state, super::colors::rgba(base_ink));
    // Cache ordinary and selected colors together: moving a selection later
    // recolors instances without repeating this cell walk or text shaping.
    let (ink, background) = policy.resolve(state, cell.raw.fg, cell.raw.bg, attrs);
    paint::add_background(
        shape.cache,
        column,
        background,
        metrics,
        policy.clear(state),
    );
    // Placeholder colours encode IDs, not ink. The paper still paints, but
    // neither glyphs nor SGR decorations may turn this metadata into tofu.
    if placeholder {
        shape.finish()?;
        return Ok(());
    }
    // Without an SGR line there is no decoration output. Do not resolve its
    // colour or enter the outlined decoration helper for every ordinary cell.
    if cell
        .raw
        .attrs
        .intersects(Attrs::UNDERLINE_MASK | Attrs::STRIKE | Attrs::OVERLINE)
    {
        decorate(
            shape,
            column,
            &cell,
            metrics,
            policy,
            state,
            ink,
            selected_ink,
        )?;
    }
    if cell
        .raw
        .flags
        .intersects(CellFlags::WIDE_SPACER | CellFlags::MULTICELL)
    {
        // A block breaks shaping across its reserved footprint. Wide spacers
        // instead remain part of their preceding ordinary cluster.
        finish_block(shape, cell.raw.flags)?;
        // Its paper and any explicit SGR line have already been emitted above.
        // Its leading cell already supplied the Unicode scalar and span;
        // shaping this placeholder would create an extra space glyph.
        return Ok(());
    }
    // SGR 8 hides ink while retaining the stored character for selection.
    // SGR 5 remains in the cache and is filtered during frame assembly.
    let style = run_style(cell.raw.attrs, ink, selected_ink);
    // Width is terminal state, including legacy attachments and clipped wide cells.
    let span = if cell.raw.flags.contains(CellFlags::WIDE) {
        2
    } else {
        1
    };
    shape.visit(column, style, cell.text, span)
}

/// Rectangles terminate ordinary shaping; wide spacers stay in their owner's run.
fn finish_block(shape: &mut RowShape<'_>, flags: CellFlags) -> Result<(), String> {
    if flags.contains(CellFlags::MULTICELL) {
        shape.finish()?;
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    /// Per-test observation has no production storage or per-cell instrumentation.
    static PLACEHOLDER_CHECKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Ordinary cells must not enter line geometry or underline-colour resolution.
    static DECORATION_CELLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Returns actual placeholder inspections on the calling test thread.
#[cfg(test)]
pub(in crate::frame_builder) fn placeholder_checks() -> usize {
    PLACEHOLDER_CHECKS.with(std::cell::Cell::get)
}

/// Resolves and emits SGR lines only for cells that actually request them.
///
/// The caller has already excluded image placeholders and cells without lines.
/// This helper must also accept spaces: underlining a blank is visible output.
/// Resolved ink is passed in so a decorated cell does not repeat color policy.
#[allow(
    clippy::too_many_arguments,
    reason = "decoration borrows existing row and resolved paint without allocating a separate context"
)]
fn decorate(
    shape: &mut RowShape<'_>,
    column: usize,
    cell: &SnapshotCell<'_>,
    metrics: CellMetrics,
    policy: &ColorPolicy,
    state: &PaintState,
    ink: [u8; 3],
    selected_ink: u32,
) -> Result<(), String> {
    #[cfg(test)]
    DECORATION_CELLS.with(|count| count.set(count.get() + 1));
    // SGR 58 lives in sparse cell extras. SGR 59 removes it, returning to
    // the resolved foreground even when inverse or dim is active.
    let underline_ink = cell
        .underline_color
        // An explicit underline color is independent of the glyph's font face.
        .map(|color| policy.resolve_color(state, color, ink))
        .unwrap_or(ink);
    // Decorations belong to the physical cell span. This includes blanks,
    // which applications often underline to show editable input regions.
    paint::add_decoration(
        shape.cache,
        column,
        ink,
        underline_ink,
        cell.raw.attrs,
        metrics,
    );
    // A curly SGR line uses one prepared Slug period per cell. Its atlas
    // shape is shared even when successive cells have different colors.
    if cell.raw.attrs.underline_style() == 3 {
        wave::add(shape.backend, shape.cache, column, underline_ink, metrics)?;
        shape
            .cache
            .wave_selection_ink
            // Keep selected colors parallel to every newly appended wave piece.
            .resize(shape.cache.waves.len(), selected_ink);
    }
    Ok(())
}

/// Counts actual decorated-cell helper entries, excluding plain cells.
#[cfg(test)]
pub(in crate::frame_builder) fn decoration_cells() -> usize {
    DECORATION_CELLS.with(std::cell::Cell::get)
}

/// Keeps run boundaries dependent on shaping/visibility, not sparse block keys.
fn run_style(attrs: Attrs, ink: [u8; 3], selected_ink: u32) -> RunStyle {
    RunStyle {
        face: font_style(attrs),
        ink,
        selection_ink: selected_ink,
        hidden: attrs.contains(Attrs::HIDDEN),
        blink: attrs.contains(Attrs::BLINK),
    }
}
