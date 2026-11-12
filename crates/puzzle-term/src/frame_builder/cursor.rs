//! Device-pixel cursor shapes above terminal ink (§10.5).
//!
//! DECSCUSR selects block, underline or bar, while DECTCEM controls
//! visibility. A focused block fills its cell and redraws the isolated
//! source glyph in `cursorText`; an unfocused window gets an outline so
//! the existing text remains legible. The VT row is not rewritten for a
//! moving cursor, and the GPU cursor batch is assembled every frame.
//!
//! Cursor width follows a wide leading cell. Its spacer has no independent
//! glyph, but the cursor must span both physical columns when the caret is
//! on the leading half. The row builder isolates this cell from ligature
//! substitutions and retains its Slug instances for the redraw layer.
//! Bitmap effects retain their affine map in the shared paint table.
//! The cursor resolves a tint variant through that table before uploads drain.
//! This preserves opacity and transformed source-size selection on bitmap ink.
//! Ordinary masks still carry their packed RGB directly in the instance.
//!
//! OSC 66 changes the cursor footprint without changing its physical VT address.
//! Any continuation cell selects the same rectangle and isolated ink owner.
//! A root above the viewport retains its original baseline through the block map,
//! while the visible extent is clipped to the grid before being reported to IME.
//! Bar and underline geometry derive from that extent, not the addressed fragment.
//! No host needs to rediscover a block or subtract its own padding origin.
//! The snapshot contains all coordinates; this code cannot consult the parser.
//! Drawing and host cursorRect therefore agree even if newer output is pending.
//! Selection and effects retain independent paint streams for the same ink.
//! Cursor drawing never allocates a new outline or reshapes a scaled block.
//! Offscreen footprints report no host rectangle and produce no visible pixels.
//! Ordinary scalar and wide-cell extents preserve the pre-OSC-66 behavior.
use super::{FrameBuilder, colors};
use crate::frame_snapshot::FrameSnapshot;
use crate::session::cursor_settings::{CursorSettings, CursorStyle};
use puzzle_frame::{Batch, PipelineKind, RectInstance, RectU32, RenderFrame, ZSlot};

impl FrameBuilder {
    /// Logical visible cursor extent used by hosts for IME candidate placement.
    pub fn cursor_extent(&self, snapshot: &FrameSnapshot) -> Option<[f32; 4]> {
        let [column, row, columns, rows] = snapshot.cursor_extent()?;
        let dpr = self.dpr;
        let x = self.grid_x() as f32 + column as f32 * self.metrics.width_px as f32;
        let y = self.padding as f32
            + row as f32 * self.metrics.height_px as f32
            + self.row_shift as f32;
        let width = columns as f32 * self.metrics.width_px as f32;
        let height = rows as f32 * self.metrics.height_px as f32;
        // Intersect the same grid used by drawing. A fully clipped cursor is null.
        let grid = self.geometry.grid.device(dpr);
        let lo = [x.max(grid.origin[0]), y.max(grid.origin[1])];
        let hi = [
            (x + width).min(grid.origin[0] + grid.size[0]),
            (y + height).min(grid.origin[1] + grid.size[1]),
        ];
        (hi[0] > lo[0] && hi[1] > lo[1]).then_some([
            lo[0] / dpr,
            lo[1] / dpr,
            (hi[0] - lo[0]) / dpr,
            (hi[1] - lo[1]) / dpr,
        ])
    }

    /// Invalidates paint caches when a cursor preference changes live.
    pub fn set_cursor_settings(&mut self, settings: CursorSettings) {
        if self.cursor_settings != settings {
            self.cursor_settings = settings;
            // Cursor ink is a transient layer, but the style epoch also
            // guarantees that a retained glyph redraw uses current color.
            self.style_epoch = self.style_epoch.wrapping_add(1);
        }
    }
}

/// Appends at most one cursor rectangle and its prepared glyph redraw.
pub(super) fn append(
    builder: &FrameBuilder,
    context: &mut super::context::TextContext,
    snapshot: &FrameSnapshot,
    frame: &mut RenderFrame,
    grid_width: u32,
    grid_height: u32,
) {
    let Some(cursor) = snapshot.cursor().filter(|state| state.view.visible) else {
        return;
    };
    // The session owner seals paint and the cursor cell in one publication,
    // so inverse RGB cannot race with a later OSC 4 update (D-50).
    let Some(paint) = snapshot.paint() else {
        return;
    };
    let presentation = builder
        .cursor_settings
        .presentation(cursor.view.style, paint.cursor_blink);
    // Session schedules the timer from this same precedence decision; a
    // forced steady caret cannot disappear on a stale blink phase.
    // Focus loss always paints a hollow location cue; a previous focused
    // blink-off phase must not hide it from the inactive pane.
    if builder.focused && !builder.blink_on && presentation.blinks {
        return;
    }
    // Cursor occupancy is sealed with its rectangle. A lower fragment uses
    // the first visible owner row, including when the root is above the viewport.
    let Some([column, row_index, columns, rows]) = snapshot.cursor_extent() else {
        return;
    };
    let metrics = builder.metrics;
    let x = builder.grid_x() as f32 + column as f32 * metrics.width_px as f32;
    let y = builder.padding as f32
        + row_index as f32 * metrics.height_px as f32
        + builder.row_shift as f32;
    let width = columns as f32 * metrics.width_px as f32;
    let height = rows as f32 * metrics.height_px as f32;
    // Focus loss changes only the cursor outline; it must not reshape rows.
    let stroke = if builder.focused { 0.0 } else { 1.0 };
    // Resolve the visible cell's colors after inverse video and contrast;
    // OSC 12 may still override the configured cursor rectangle color.
    let (cursor_color, text_color) =
        builder
            .colors
            .cursor_pair(paint, builder.cursor_settings.color, cursor.cell);
    let rect = cursor_rect(
        presentation.style,
        x,
        y,
        width,
        height,
        stroke,
        cursor_color,
    );
    // Grid clipping prevents a cursor at the margin from painting padding.
    let clip = Some(builder.grid_clip(frame.target_size, grid_width, grid_height));
    paint_cursor(frame, rect, clip);
    // Only filled blocks cover their source ink. A bar, underline or hollow
    // outline leaves the already drawn glyph visible without another draw.
    if builder.focused && presentation.style == CursorStyle::Block {
        redraw_cell(builder, frame, row_index, y, clip, text_color);
        super::raster::cursor(
            builder,
            &mut context.backend,
            frame,
            row_index,
            y,
            clip,
            text_color,
        );
    }
}

/// Adds the caret paper in its slot before isolated cursor ink.
fn paint_cursor(frame: &mut RenderFrame, rect: RectInstance, clip: Option<RectU32>) {
    let first_rect = frame.rects.len() as u32;
    frame.rects.push(rect);
    frame.batches.push(Batch {
        // Cursor is above the text slot, independent of row-cache ordering.
        slot: ZSlot::Cursor,
        pipeline: PipelineKind::Rect,
        instances: first_rect..first_rect + 1,
        scissor: clip,
    });
}

/// Repaints isolated cell ink above a filled block in cursorText color.
fn redraw_cell(
    builder: &FrameBuilder,
    frame: &mut RenderFrame,
    row_index: usize,
    y: f32,
    clip: Option<RectU32>,
    text_color: [u8; 3],
) {
    let first_slug = frame.slugs.len() as u32;
    // The row cached the cursor's isolated glyph so a filled block can
    // recolor it without another shaping or outline-preparation pass.
    for (original, palette) in builder.rows[row_index]
        .cursor_glyphs
        .iter()
        .zip(&builder.rows[row_index].cursor_palette)
    {
        let mut glyph = *original;
        // Cached instances are row-relative; the grid inset and row
        // origin are applied only while assembling this transient layer.
        glyph.t[0] += builder.grid_x() as f32;
        glyph.t[1] += y;
        // Fixed CPAL fills preserve their authored color when the caret covers them.
        if !*palette {
            // Preserve COLR paint alpha while changing only foreground RGB.
            glyph.color = (colors::rgba(text_color) & 0x00ffffff) | (glyph.color & 0xff000000);
        }
        frame.slugs.push(glyph);
    }
    // The redraw needs a batch after the cursor rectangle so the inverted
    // glyph remains visible instead of being covered by the block.
    frame.batches.push(Batch {
        slot: ZSlot::Cursor,
        pipeline: PipelineKind::Slug,
        instances: first_slug..frame.slugs.len() as u32,
        scissor: clip,
    });
}

/// Converts the effective setting to a snapped bar or filled block.
fn cursor_rect(
    style: CursorStyle,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    hollow_stroke: f32,
    color: [u8; 3],
) -> RectInstance {
    let (rect, stroke_width) = if hollow_stroke > 0.0 || style == CursorStyle::BlockHollow {
        // Focus loss uses a hollow block regardless of the active caret
        // style, matching the visual focus cue in §10.5.
        ([x, y, width, height], hollow_stroke)
    } else {
        // The Session timer has already resolved blinking; geometry can
        // follow the requested shape without inspecting VT numeric codes.
        match style {
            CursorStyle::Underline => ([x, y + height - 1.0, width, 1.0], 0.0),
            CursorStyle::Bar => ([x, y, 1.0, height], 0.0),
            CursorStyle::Block | CursorStyle::BlockHollow => ([x, y, width, height], 0.0),
        }
    };
    // Zero corner radius and pixel-sized dimensions keep the thin caret
    // aligned with the same device-pixel grid as builtin box drawing.
    RectInstance {
        rect,
        color: colors::rgba(color),
        corner_radius: 0.0,
        stroke_width,
        flags: 0,
    }
}
