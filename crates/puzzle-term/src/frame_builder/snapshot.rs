//! Snapshot-only frame building with no access to the Session owner.
//!
//! Dimensions are validated independently of row preparation. Cache slots
//! follow LineId before misses are rebuilt, preserving recently visible ink
//! while keeping storage bounded to twice the visible grid height (P10).
//!
//! Rotating slots retain all visible cells, but copy only rows changed since
//! their last fill. Renderer cache history is separate: identities/versions
//! decide rebuilds even when a slot did not copy that row this turn. Cursor
//! isolation remains part of the cache key even without content changes.
//!
//! The spare cache keeps recently hidden rows, never the entire scrollback.
//! Matching all visible identities before rebuilding any miss avoids losing
//! a row whose cache currently occupies the newly exposed row's screen slot.
//! Glyph uploads are collected after rebuilding, before any draw references
//! a freshly prepared atlas placement.
//!
//! A reused row still stamps its shape batch with the actual frame ordinal.
//! Opt-in observers separate cache matching, row work, assembly, geometry,
//! providers and uploads without selecting a different rendering algorithm.
//! An ordinary build supplies an empty generic observer, removing timer costs.
//! The row phase includes cold bank warming; subsequent exposed history rows
//! keep both their parallel ink buffers and their atlas dependency allocation.

use super::{FrameBuilder, context::TextContext, cursor, row, uploads};
use crate::frame_snapshot::FrameSnapshot;
use puzzle_frame::RenderFrame;
/// Snapshot preparation uses the same cache and assembly as direct VT frames.
impl FrameBuilder {
    /// Accepts publication ownership before building from the sealed source values.
    /// Hosts skipping a slot call accept_snapshot_views before returning it instead.
    pub fn build_published(
        &mut self,
        context: &mut TextContext,
        snapshot: &mut FrameSnapshot,
        frame: &mut RenderFrame,
    ) -> Result<(), String> {
        self.accept_snapshot_views(snapshot);
        // The host owns the context; publication never begins or mutates glyph work.
        context.begin_frame()?;
        self.sync_generation(context.atlas_generation());
        self.build_snapshot(context, snapshot, frame)
    }

    /// Builds neutral geometry using only copied VT state.
    pub fn build_snapshot(
        &mut self,
        context: &mut TextContext,
        snapshot: &FrameSnapshot,
        frame: &mut RenderFrame,
    ) -> Result<(), String> {
        self.build_snapshot_observed(context, snapshot, frame, |_| {})
    }

    /// Builds the identical frame with opt-in phase boundaries for native diagnostics.
    /// The observer is monomorphized away for ordinary builds; it must not mutate ink.
    pub(super) fn build_snapshot_inner(
        &mut self,
        context: &mut TextContext,
        snapshot: &FrameSnapshot,
        frame: &mut RenderFrame,
        mut observe: impl FnMut(super::BuildPhase),
    ) -> Result<(), String> {
        // Rendering samples no live preferences; this slot owns the source policy.
        if let Some(policy) = &snapshot.policy {
            self.apply_published_policy(policy);
        }
        if self.screen_epoch != snapshot.screen_epoch() {
            // A fresh screen context cannot reuse equal ID/version pairs.
            self.style_epoch = self.style_epoch.wrapping_add(1);
            self.screen_epoch = snapshot.screen_epoch();
        }
        self.row_shift = snapshot.row_shift();
        let (grid_width, grid_height) = self.initialize_frame(snapshot, frame)?;
        self.prepare_cache_order(context, snapshot);
        // All reused rows must pin their raster pages before any changed row inserts ink.
        super::raster::pin_cached(self, context, snapshot);
        let raster_generation = context.raster.generation();
        observe(super::BuildPhase::CacheOrder);
        self.rebuild_snapshot_rows(context, snapshot)?;
        // Warming a grid includes its spare bank; familiar history is steady-state work.
        self.warm_spares(context);
        observe(super::BuildPhase::Rows);
        // Uploads follow all row rebuilds so a newly prepared glyph has
        // its texels on the GPU before the draw that references its handle.
        self.assemble(snapshot, frame, grid_width, grid_height);
        cursor::append(self, context, snapshot, frame, grid_width, grid_height);
        observe(super::BuildPhase::Assembly);
        self.update_geometry(snapshot, frame.target_size);
        observe(super::BuildPhase::Geometry);
        self.append_overlays(context, snapshot, frame, grid_width, grid_height);
        if let Some(images) = &mut frame.images {
            // Overlay rasters use the same independently accounted image allowance.
            // Forward it even when no kitty placement was visible in this publication.
            images.budget = snapshot.image_budget;
            // The ordinary painter order is already correct without image primitives.
            // A retired image keeps this allocation, but empty sources need no pass.
            if !images.sources.is_empty() {
                frame.order_batches();
            }
        }
        observe(super::BuildPhase::Overlays);
        // Unchanged row caches still draw these paints, so refresh their ages.
        for draw in &frame.slugs {
            context.backend.mark_color_used(draw.paint);
        }
        super::raster::uploads(context, frame)?;
        uploads::append(frame, context.backend.atlas_mut());
        for (first, colors) in context.backend.take_paint_uploads() {
            // Pending frame uploads survive hidden, lost or skipped presentations.
            frame
                .uploads
                .push(puzzle_frame::Upload::Paint { first, colors });
        }
        // In-build eviction leaves this frame valid but invalidates other sessions' caches.
        context.finish_raster_build(raster_generation);
        observe(super::BuildPhase::Uploads);
        Ok(())
    }

    /// Rebuilds only exposed or damaged rows, then stamps their retained atlas usage.
    fn rebuild_snapshot_rows(
        &mut self,
        context: &mut TextContext,
        snapshot: &FrameSnapshot,
    ) -> Result<(), String> {
        self.rebuilt_rows = 0;
        // Reuse row-local geometry when only its screen position changes.
        for index in 0..snapshot.rows() {
            let meta = snapshot
                .row_meta(index)
                .ok_or("visible row metadata missing")?;
            let cache = &mut self.rows[index];
            // Identity catches scrollback rotation, version catches edits,
            // and epoch catches changes to colors or font policy. Checking
            // only one of these can display stale text in a valid cell.
            let changed = cache.id != meta.id
                || cache.version != meta.version
                || cache.block_signature != meta.block_signature
                || cache.style_epoch != self.style_epoch
                || cache.atlas_generation != context.atlas_generation()
                || cache.cursor_column != meta.cursor_column;
            if changed {
                let row = snapshot
                    .retained_row(index)
                    .ok_or("changed row not copied")?;
                row::rebuild(
                    self,
                    context,
                    row,
                    snapshot.paint().ok_or("snapshot has no paint state")?,
                    index,
                    meta.cursor_column,
                )?;
                // The sealed row summary excludes sparse lookup/paint work entirely
                // from ordinary rows, including after the final block was erased.
                if meta.block_signature != 0 {
                    super::multicell::append(self, context, snapshot, index)?;
                }
                self.rows[index].block_signature = meta.block_signature;
                self.rebuilt_rows += 1;
            }
            // Even unchanged rows draw this frame. One stamp covers all cached dependencies.
            let cache = &mut self.rows[index];
            context.backend.use_row_batch(
                &mut cache.shape_batch,
                cache
                    .glyphs
                    .iter()
                    .chain(&cache.waves)
                    .map(|draw| (draw.glyph_loc, draw.paint)),
                changed,
            );
        }
        Ok(())
    }

    /// Validates physical extents and resets reused output arrays.
    fn initialize_frame(
        &self,
        snapshot: &FrameSnapshot,
        frame: &mut RenderFrame,
    ) -> Result<(u32, u32), String> {
        let rows = snapshot.grid_rows();
        let columns = snapshot.columns();
        // The product is checked before adding the fixed outer padding;
        // an untrusted resize must not wrap to a tiny GPU surface.
        let grid_width = u32::try_from(columns)
            .map_err(|_| "grid width exceeds device range")?
            .checked_mul(self.metrics.width_px)
            .ok_or("grid pixel width overflow")?;
        let grid_height = u32::try_from(rows)
            .map_err(|_| "grid height exceeds device range")?
            .checked_mul(self.metrics.height_px)
            .ok_or("grid pixel height overflow")?;
        // `clear_instances` retains all vector capacities. A changed row
        // can be assembled into those buffers without fresh heap requests.
        let target_width = grid_width
            .checked_add(self.padding.saturating_mul(2).saturating_add(
                u32::from(self.gutter_columns).saturating_mul(self.metrics.width_px),
            ))
            .filter(|width| *width <= 8192)
            .ok_or("grid target width exceeds GPU limit")?;
        let target_height = grid_height
            .checked_add(self.padding.saturating_mul(2))
            .filter(|height| *height <= 8192)
            .ok_or("grid target height exceeds GPU limit")?;
        frame.clear_instances();
        // Sixteen pixels on each edge expose glyph overhang in harness
        // screenshots and keep the grid scissor inside the GPU target.
        // The harness sizes its offscreen target to the padded grid. A
        // presentable window uses its exact backing pixels so the GPU never
        // scales terminal cell geometry while resizing (§13.1).
        frame.target_size = self.surface_size.unwrap_or([target_width, target_height]);
        let paint = snapshot.paint().ok_or("snapshot has no paint state")?;
        frame.clear = self.colors.clear(paint);
        frame.globals.dpr = self.dpr;
        Ok((grid_width, grid_height))
    }
    /// Reorders cache ownership before any miss can overwrite needed ink.
    fn prepare_cache_order(&mut self, context: &mut TextContext, snapshot: &FrameSnapshot) {
        let rows = snapshot.rows();
        // Fractional movement adds an edge row, not a grid resize. Always keep
        // both cache banks for grid height + one: truncating on integral frames
        // drops warmed vectors and atlas tokens, then allocates on the next edge.
        // Actual grid resizing still bounds storage to twice its maximum slice.
        let bank = snapshot.grid_rows().saturating_add(1);
        // A real grid shrink retires dropped tokens too; they must not accumulate
        // in the shared atlas after their sole row-cache owner has disappeared.
        for row in self.rows.iter().skip(bank.saturating_mul(2)) {
            context.backend.atlas_mut().forget_batch(row.shape_batch);
        }
        self.ensure_rows(bank);
        self.visible_rows = rows;
        // Reorder all matches before rebuilding misses. Otherwise a newly
        // exposed row could overwrite ink needed at a later screen position.
        for index in 0..rows {
            let id = snapshot.row_meta(index).and_then(|meta| meta.id);
            if let Some(found) = self.rows.iter().position(|row| row.id == id) {
                self.rows.swap(index, found);
            }
        }
        // Keep recently offscreen rows in a bounded spare prefix. Returning
        // one row toward the previous viewport can reuse its prepared ink.
        for index in 0..rows {
            let id = snapshot.row_meta(index).and_then(|meta| meta.id);
            if self.rows[index].id != id
                && let Some(empty) = self.rows[rows..].iter().position(|row| row.id.is_none())
            {
                self.rows.swap(index, rows + empty);
            }
        }
    }
}
