//! Cached overlay outlines and gradient rows in the shared glyph stores (§11.2).
//!
//! Providers retain immutable Arc paths. Weak owners give them a collision-free
//! identity without a global counter: a dead weak reference keeps its allocation
//! address reserved until pruning. A context owns all atlas IDs, including COLR
//! and overlay chunks, so several Sessions cannot collide by choosing equal keys.
//! Translation never affects prepared geometry; rotation, zoom and DPR do affect
//! the device-space partitions and therefore belong to the variant cache key.
//!
//! Atlas restoration does not repeat conversion or clipping. Retained quadratics
//! and bands supply the same chunks when a generational address expires.
//! A chunk's ownership rectangle is separate from its guarded contour bounds:
//! the former prevents double blending, the latter preserves seam coverage.
//! Brushes stay in logical coordinates until preparation maps their inverse
//! into the baked device coordinates evaluated by the Slug fragment shader.
use super::SlugBackend;
use kurbo::{Affine, BezPath, Rect};
use puzzle_slug::{ShapeHandle, ShapeKey, atlas::FillRule, paint::table::PaintHandle};
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

/// Physical outline and pixel ownership, ready for a translated Slug instance.
#[derive(Clone, Debug)]
pub struct OverlayChunk {
    /// Stable address, recreated only if its whole page was evicted.
    pub shape: ShapeHandle,
    /// Device-pixel rectangle, before the instance's translation.
    pub owner: Rect,
}

/// CPU geometry survives atlas eviction; only shader addresses need refreshing.
struct Paths {
    /// Immutable source owns the cache identity while any draw still uses it.
    source: Weak<BezPath>,
    /// Bounded pieces retain quadratics and band lists for rare atlas restoration.
    pieces: Vec<(ShapeKey, puzzle_slug::chunks::Chunk)>,
    /// Reused returned instance descriptors avoid a per-frame collection.
    draws: Vec<OverlayChunk>,
}

/// Gradient address and source owner, tracked independently of path lifetime.
struct Brush {
    /// Dead sources permit bounded cold pruning without scanning font caches.
    source: Weak<puzzle_slug::paint::Paint>,
    /// Generational shader table address.
    handle: PaintHandle,
}

/// Shared-context cache key: live allocation identity and exact linear coefficients.
type Key = (usize, [u64; 4]);

/// Overlay caches use existing texture budgets and bounded CPU descriptor counts.
#[derive(Default)]
pub(super) struct Cache {
    /// Shape variants, independent of logical provider keys and translations.
    paths: HashMap<Key, Paths>,
    /// Brushes mapped into the same physical root coordinates as their chunks.
    brushes: HashMap<Key, Brush>,
}

/// Shape and paint preparation stays synchronous on the frame-building thread.
impl SlugBackend {
    /// Prepares one translation-free physical variant and marks every chunk live.
    pub fn prepare_overlay(
        &mut self,
        path: &Arc<BezPath>,
        linear: Affine,
    ) -> Result<&[OverlayChunk], String> {
        // Translation belongs to instances, never to a cached physical partition.
        let linear = linear.with_translation(kurbo::Vec2::ZERO);
        // Pointer identity is stable while the Weak owner reserves the allocation.
        // Exact coefficient bits avoid approximate-transform cache collisions.
        let key = key(path, linear);
        if !self.overlays.paths.contains_key(&key) {
            // Pruning is cold-only; a warmed frame never scans every source owner.
            // Still-live paths retain variants even when another Session uses them.
            self.overlays
                .paths
                .retain(|_, entry| entry.source.strong_count() != 0);
            if self.overlays.paths.len() >= 4096 {
                // GPU texture budgets alone do not bound retained CPU geometry.
                return Err("overlay cache reached 4096 device variants".into());
            }
            // The splitter validates both spatial size and actual band membership.
            // Provider-supplied paths cannot publish an unbounded shader loop.
            let chunks = puzzle_slug::chunks::prepare(path, linear)?;
            let mut pieces = Vec::with_capacity(chunks.len());
            for chunk in chunks {
                // One allocator spans normal glyphs, COLR clips and overlay shapes.
                // Equal provider-local keys therefore never alias an atlas entry.
                self.next_shape = self
                    .next_shape
                    .checked_add(1)
                    .ok_or("overlay shape identity exhausted")?;
                pieces.push((ShapeKey(self.next_shape), chunk));
            }
            self.overlays.paths.insert(
                key,
                Paths {
                    source: Arc::downgrade(path),
                    pieces,
                    draws: Vec::new(),
                },
            );
        }
        let Some(entry) = self.overlays.paths.get_mut(&key) else {
            // Return a report rather than assuming a cache lookup always succeeds.
            return Err("overlay variant disappeared during preparation".into());
        };
        // A warm path only touches retention clocks; it rebuilds neither bands nor texels.
        for (id, chunk) in &entry.pieces {
            if self.atlas.get(*id).is_none() {
                // Only an expired atlas address needs owned geometry for repacking.
                // Cloning cold band lists leaves the retained CPU source available.
                self.atlas
                    .prepare_quadratics(
                        *id,
                        chunk.shape.clone(),
                        chunk.bands.clone(),
                        FillRule::NonZero,
                        self.colors.frame,
                    )
                    .map_err(|e| e.to_string())?;
            }
            self.atlas.mark_used(*id, self.colors.frame);
        }
        entry.draws.clear();
        // Retain the descriptor allocation across animated translations (P1).
        // Collect addresses after preparation; insertion never clears a live page.
        for (id, chunk) in &entry.pieces {
            let shape = self
                .atlas
                .get(*id)
                .ok_or("live overlay chunk evicted while preparing its siblings")?;
            entry.draws.push(OverlayChunk {
                shape,
                owner: chunk.owner,
            });
        }
        Ok(&entry.draws)
    }

    /// Maps a retained logical brush into device root coordinates and shares COLR rows.
    pub fn prepare_overlay_paint(
        &mut self,
        paint: &Arc<puzzle_slug::paint::Paint>,
        linear: Affine,
    ) -> Result<u32, String> {
        // The same translation-free convention applies to cached brush coordinates.
        let linear = linear.with_translation(kurbo::Vec2::ZERO);
        // The logical brush owner and device linear map completely identify a row.
        let key = key(paint, linear);
        if let Some(entry) = self.overlays.brushes.get(&key)
            && self.colors.paints.contains(entry.handle)
        {
            // Paint eviction happens before frame capture; hits validate generation.
            // Marking a reused row protects it when other Sessions fill the table.
            let index = entry.handle.index;
            self.mark_color_used(index);
            return Ok(index);
        }
        self.overlays
            .brushes
            .retain(|_, entry| entry.source.strong_count() != 0);
        if self.overlays.brushes.len() >= 4096 {
            // Descriptor growth remains bounded even when the table rejects inserts.
            return Err("overlay brush cache reached 4096 variants".into());
        }
        if linear.determinant().abs() < 1e-12 || !paint.clips.is_empty() {
            // Singular maps have no inverse; raw clip handles belong to another owner.
            return Err(
                "overlay brush needs an invertible transform and no raw atlas clips".into(),
            );
        }
        let mut mapped = (**paint).clone();
        // Clone stops only on a cold variant; animation hits keep the retained row.
        // The shader sees baked physical coordinates, while brush geometry stays logical.
        mapped.inverse = (Affine::new(paint.inverse.map(f64::from)) * linear.inverse())
            .as_coeffs()
            .map(|v| v as f32);
        let handle = self.color_paint(&mapped)?;
        // Publish a handle only after the shared paint table accepts the whole row.
        self.overlays.brushes.insert(
            key,
            Brush {
                source: Arc::downgrade(paint),
                handle,
            },
        );
        Ok(handle.index)
    }
}

/// Excludes translation because it is supplied on each instance at drawing time.
fn key<T>(source: &Arc<T>, linear: Affine) -> Key {
    // The caller supplies translation separately, so only four values affect bands.
    let coeffs = linear.as_coeffs();
    (
        Arc::as_ptr(source) as usize,
        [
            coeffs[0].to_bits(),
            coeffs[1].to_bits(),
            coeffs[2].to_bits(),
            coeffs[3].to_bits(),
        ],
    )
}
