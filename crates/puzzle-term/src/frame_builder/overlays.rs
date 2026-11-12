//! Frozen overlay callbacks and logical-to-device painter merge (design §10.4).
//!
//! View callbacks use immutable model values and captured declaration samples.
//! Presented region tables preserve the exact grid scissor for input picking.
//!
//! A late publication transfers new view registrations and a bounded read-only
//! buffer window into this builder. Views then see fresh placement geometry
//! after row shaping, while the desktop parser runs independently. Shapes,
//! text and atlas uploads stay on the host's frame-building thread (D-29).
//!
//! Overlay callbacks need current placements, so they run after every dirty
//! row has been shaped and cached. Geometry is assembled from cached neutral
//! metadata for unchanged rows too. It records one glyph per placement, not
//! one per synthetic bold draw or COLR layer.
//!
//! Physical row offsets are applied before conversion to logical px. The
//! fractional scroll displacement is therefore shared by text, cell hits
//! and decorations; providers do not reconstruct scrolling from row IDs.
//! Padding and exact host surface extent remain distinct from grid bounds.
//!
//! Model callbacks never move into the builder. Their immutable versions and
//! one-time view registrations arrive with source snapshots. Parser events
//! remain pending in Session; stable IDs preserve registrations across skips.
//! No callback can borrow the parser while fonts and atlases are being used.
//!
//! Each of the seven z-slots has independent nested scissors. Grid slots
//! begin with the grid scissor, whereas Top panels can occupy padding. Clip
//! stacks retain capacity across animations. Painter order within a slot is
//! registration order, then command order; clipping in one provider cannot
//! leak into the next. Stable batch ordering uses no heap sort scratch.
//!
//! Draw lists survive idle frames, so callbacks are not invoked for cursor
//! blinking or unrelated grid damage. Anchors can still change position
//! independently of a provider's content. Atlas uploads are collected only
//! after all primitives have been prepared by the shared context.
//! Sparse OSC 66 hit extents travel beside ordinary grid geometry in this publication.
use super::FrameBuilder;
/// Ink-only variants rebuild affected glyphs without changing retained neutral rows.
pub(super) mod effects;
/// Affine ink maps preserve neutral source placements.
pub(super) mod ink;
/// Scissor stacks and painter-order batches shared by every overlay primitive.
mod paint;
/// Shared-atlas shapes with guarded geometry and disjoint device scissors.
mod shapes;
/// Sized annotations share natural layout and bitmap effect preparation.
pub(super) mod text;
/// Annotation measurement and common alpha policy.
mod text_metrics;
use puzzle_frame::{RectU32, RenderFrame, ZSlot};
use puzzle_overlay::{
    geometry::{Line, Rect, TextGeometry},
    view::ViewCtx,
    view_registry::ViewRegistry,
};

/// Drawing caches and frozen source state are separate from interaction models.
#[derive(Default)]
pub(crate) struct OverlayFrame {
    /// Drawing callbacks remain with this builder, never the interaction registry.
    pub registry: ViewRegistry,
    /// Session-owned path descriptors outlive retained provider draw lists.
    shapes: puzzle_overlay::shapes::ShapeCache,
    /// Nested clip rectangles keep capacity across animated rebuilds.
    clips: Vec<RectU32>,
    /// At most one active effect variant per visible row, with reusable capacity.
    effects: effects::Cache,
}
/// Fresh grid placements precede callback execution and slot compilation.
impl FrameBuilder {
    /// Returns the current device scale even before the first placement build.
    pub fn overlay_dpr(&self) -> f32 {
        self.dpr
    }
    /// Borrows this frame's placement geometry, independent of its glyph backend.
    pub fn text_geometry(&self) -> &TextGeometry {
        &self.geometry
    }
    /// Copies neutral placements from row caches into reused logical-space queries.
    pub(super) fn update_geometry(
        &mut self,
        snapshot: &crate::frame_snapshot::FrameSnapshot,
        target: [u32; 2],
    ) {
        let dpr = self.dpr;
        self.geometry.lines.clear();
        self.geometry.placements.clear();
        self.geometry.blocks.clear();
        self.geometry.cell = [
            self.metrics.width_px as f32 / dpr,
            self.metrics.height_px as f32 / dpr,
        ];
        self.geometry.inset = self.padding as f32 / dpr;
        self.geometry.columns = snapshot.columns();
        self.geometry.dpr = dpr;
        self.geometry.viewport = Rect {
            origin: [0.0; 2],
            size: target.map(|value| value as f32 / dpr),
        };
        self.update_grid_clip(snapshot, target);
        super::multicell::geometry(self, snapshot);
        for (index, row) in self.rows.iter().take(self.visible_rows).enumerate() {
            let Some(id) = row.id else {
                continue;
            };
            let offset = [
                self.grid_x() as f32,
                self.padding as f32
                    + index as f32 * self.metrics.height_px as f32
                    + self.row_shift as f32,
            ];
            self.geometry.lines.push(Line {
                version: row.version,
                id,
                y: offset[1] / dpr,
            });
            for source in &row.placements {
                let mut glyph = *source;
                glyph.cell.line = id;
                for (axis, shift) in offset.iter().enumerate() {
                    glyph.baseline[axis] = (glyph.baseline[axis] + shift) / dpr;
                    glyph.ink.origin[axis] = (glyph.ink.origin[axis] + shift) / dpr;
                    glyph.ink.size[axis] /= dpr;
                }
                self.geometry.placements.push(glyph);
            }
        }
    }
    /// Copies the exact device scissor to logical geometry for region picking.
    fn update_grid_clip(
        &mut self,
        snapshot: &crate::frame_snapshot::FrameSnapshot,
        target: [u32; 2],
    ) {
        // Hit picking shares shrinking-surface clipping and the actual grid row count.
        let clip = self.grid_clip(
            target,
            snapshot.columns() as u32 * self.metrics.width_px,
            snapshot.grid_rows() as u32 * self.metrics.height_px,
        );
        self.geometry.grid = Rect {
            origin: [clip.x as f32 / self.dpr, clip.y as f32 / self.dpr],
            size: [clip.width as f32 / self.dpr, clip.height as f32 / self.dpr],
        };
    }
    /// Runs callbacks only when needed, then merges the retained logical commands.
    pub(super) fn append_overlays(
        &mut self,
        context: &mut super::context::TextContext,
        snapshot: &crate::frame_snapshot::FrameSnapshot,
        frame: &mut RenderFrame,
        width: u32,
        height: u32,
    ) {
        let overlays = &mut self.overlays;
        if overlays.registry.is_empty() {
            return;
        }
        // Only the sealed slot supplies source state; the builder retains drawing caches.
        // Direct VT probes have an empty payload and therefore no overlay callbacks.
        let source = &snapshot.overlays;
        if let Some(buffer) = &source.buffer {
            // Providers borrow frozen contents; callbacks cannot reach the live parser.
            let terminal = buffer.terminal();
            let layouter = text::ProviderLayouter::new(context);
            // The view context has drawing resources but no Requests or Waker.
            // Current placements come from the rows just built, not prior hit geometry.
            let ctx = ViewCtx {
                geometry: &self.geometry,
                buffer: terminal.buffer(),
                time: source.time,
                theme: &source.theme,
                reduced_motion: source.reduced_motion,
                shapes: &overlays.shapes,
                text: Some(&layouter),
                decorations: &source.decorations,
                models: &source.models,
            };
            // Registry mutation changes only reconstructible lists and source versions.
            overlays
                .registry
                .build(&source.snapshot, &ctx, terminal, source.pointer);
        }
        if let Err(error) = effects::apply(self, context, snapshot, frame, width, height) {
            tracing::warn!(event = "overlay.effect_rejected", %error, "grid effect skipped");
        }
        let grid = self.grid_clip(frame.target_size, width, height);
        let overlays = &mut self.overlays;
        // Callback output is now current; effect variants are assembled before overlay ink.
        // Each slot has an independent scissor stack; Top remains outside the grid.
        for slot in [
            ZSlot::Background,
            ZSlot::BelowText,
            ZSlot::Text,
            ZSlot::TextRaster,
            ZSlot::Cursor,
            ZSlot::AboveText,
            ZSlot::Top,
        ] {
            for (_, list) in overlays.registry.lists() {
                overlays.clips.clear();
                let base = (slot != ZSlot::Top).then_some(grid);
                for draw in list.draws.iter().filter(|draw| draw.slot == slot) {
                    paint::command(
                        context,
                        frame,
                        draw,
                        &self.geometry,
                        &mut overlays.clips,
                        base,
                    );
                }
            }
        }
        paint::order_batches(&mut frame.batches);
    }
}

/// Font-pack annotations render before asynchronous full-font takeover.
mod text_pack;

/// Presentation results expose values, never the view registry or drawing callbacks.
impl FrameBuilder {
    /// Receives one-time views even from a publication that will not be built.
    /// Accept before returning a skipped slot; the model no longer owns these views.
    pub fn accept_snapshot_views(&mut self, snapshot: &mut crate::frame_snapshot::FrameSnapshot) {
        self.overlays
            .registry
            .accept(&mut snapshot.overlays.snapshot);
    }

    /// Captures the hit table of this build into reusable host-owned storage.
    /// The host sends it only after successful submission; failed builds retain old hits.
    pub fn capture_presented_regions(
        &self,
        target: &mut puzzle_overlay::regions::PresentedRegions,
    ) {
        target.capture(self.text_geometry(), self.overlays.registry.lists());
    }
}
