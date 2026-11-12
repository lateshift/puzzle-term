//! Only visible placements cross the Session boundary, with immutable source pixels.
//!
//! Tracked anchors are resolved on the owner thread. Signed row offsets include
//! images entering from history above the viewport; rendering never consults VT.
//!
//! A source remains valid when the parser later deletes or replaces its wire ID:
//! this publication holds the immutable decoded allocation, not a store lookup.
//! Hidden sources and other-screen placements never cross this ownership seam.
//! Natural-pixel geometry and explicitly cell-sized geometry need different
//! visibility calculations after the host changes font size or display scale.
//!
//! User folding changes only the viewport's row projection. Every image path
//! borrows that same source-row function during publication: physical placements
//! are translated/clipped into visible runs; placeholders use their mapped row.
//! No extra display map survives in a snapshot, and no image-free publication
//! traverses hidden source or allocates projection storage. Contiguous tests and
//! ordinary live views use the same placement admission and sorting code.
//! Images retain their independent configured residency allowance; a projected
//! run shares its existing pixel allocation rather than creating a new bitmap.
use crate::{frame_snapshot::FrameSnapshot, images::Store};
use puzzle_frame::ImageSource;
use puzzle_vt::{Terminal, tracked::PositionState};

/// One visible source and its cell-relative placement, sealed with the grid.
pub(crate) struct Placement {
    /// Shared decoded bytes keep deletion/retransmission safe across publication.
    pub source: ImageSource,
    /// Wire ID breaks equal-z ties as required by the kitty protocol.
    pub id: u32,
    /// Private transmission generation distinguishes reused wire IDs.
    pub generation: u64,
    /// Tracked column and signed row relative to this viewport's top row.
    pub anchor: [i64; 2],
    /// Source crop in physical image pixels.
    pub crop: [u32; 4],
    /// Physical offset inside the tracked cell.
    pub offset: [u32; 2],
    /// Original c/r request; zero dimensions derive from source aspect ratio.
    pub cells: [u32; 2],
    /// Original z controls the three image layers and within-layer ordering.
    pub z: i32,
    /// Placeholder row/column selects one cell-sized tile from this prototype.
    pub tile: Option<[u32; 2]>,
    /// User projection clips a physical placement to one contiguous source run.
    pub clip_rows: Option<[i64; 2]>,
}
/// Lazy snapshot storage, reusing its placement allocation on every capture.
#[derive(Default)]
pub(crate) struct Images {
    /// GPU residency is separate, with the same configured byte allowance.
    pub budget: usize,
    /// Only intersecting placements hold source pixels in a published frame.
    pub placements: Vec<Placement>,
}
/// Source ownership is resolved before publishing, alongside copied VT rows.
impl FrameSnapshot {
    #[cfg(test)]
    /// Contiguous fixture capture uses the same production implementation.
    pub(crate) fn capture_images(&mut self, store: Option<&Store>, term: &Terminal, top: usize) {
        self.capture_images_from(store, term, top, false, &|row| top.checked_add(row));
    }

    /// Borrow the publication's exact source mapping; no index vector is retained.
    pub(crate) fn capture_view_images(
        &mut self,
        store: Option<&Store>,
        term: &Terminal,
        viewport: &crate::viewport::Viewport,
    ) {
        self.capture_images_from(
            store,
            term,
            viewport.top_index(),
            viewport.projected(),
            // A UI marker has no application image, even when its reused source
            // identity was covered by a placement. Later source runs still draw.
            &|row| {
                (!viewport.marker_row(row))
                    .then(|| viewport.source_row(row))
                    .flatten()
            },
        );
    }

    /// Ordinary and projected captures share source ownership and ordering.
    fn capture_images_from(
        &mut self,
        store: Option<&Store>,
        term: &Terminal,
        top: usize,
        projected: bool,
        source_row: &impl Fn(usize) -> Option<usize>,
    ) {
        // Forward zero on disable even when the placement Vec was allocated earlier.
        self.image_budget = store.map_or(0, Store::limit);
        if let Some(images) = &mut self.images {
            images.placements.clear();
        }
        let Some(store) = store else {
            return;
        };
        for placement in store
            .placements()
            .iter()
            .filter(|p| !p.virtual_ && p.screen == term.active)
        {
            let Some((column, row, image)) = resolve(store, term, placement) else {
                continue;
            };
            // Both aspect-derived and natural heights use today's cell metrics.
            let height = placement_height(placement, term);
            if projected {
                self.capture_projected_image(placement, image, [column, row, height], source_row);
                continue;
            }
            // Include the fractional edge row; the GPU scissor rejects a zero-area touch.
            if row > top.saturating_add(self.grid_rows) || row.saturating_add(height) <= top {
                continue;
            }
            // Resolve one immutable generation before the next parser slice can replace it.
            if let Some(sealed) = seal(placement, image, column, row as i64 - top as i64) {
                self.images
                    .get_or_insert_with(Default::default)
                    .placements
                    .push(sealed);
            }
        }
        self.capture_placeholders(store, term, source_row);
        if let Some(images) = &mut self.images {
            images.budget = store.limit();
            // Unstable sorting needs no scratch allocation. Exact equal ID/z ties
            // are protocol-undefined; generation distinguishes reused IDs and spatial
            // ties put adjacent placeholder cells together for rectangular batching.
            images
                .placements
                .sort_unstable_by_key(|p| (p.z, p.id, p.generation, p.anchor[1], p.anchor[0]));
        }
    }
}

/// Resolve one live anchor and immutable source while the Session owns both.
fn resolve<'a>(
    store: &'a Store,
    term: &Terminal,
    placement: &crate::images::Placement,
) -> Option<(usize, usize, &'a crate::images::Image)> {
    // Tracked handles already encode reflow, region scrolls and screen domains.
    let PositionState::Present { line, column } = term.tracked.get(placement.anchor) else {
        return None;
    };
    // Ring order can differ from numeric LineId order after partial-region scrolls.
    let row = term.screen().ring.index_of(line)?;
    // Transmission generations prevent a reused wire ID selecting old pixels.
    let image = store
        .images()
        .iter()
        .find(|i| i.generation == placement.image)?;
    Some((column, row, image))
}

/// Visibility derives from the original c/r request after a font or DPR change.
fn placement_height(placement: &crate::images::Placement, term: &Terminal) -> usize {
    let cell = [
        u32::from(term.cell_pixels.0).max(1),
        u32::from(term.cell_pixels.1).max(1),
    ];
    let size = crate::images::geometry::display_size(
        [placement.source[2], placement.source[3]],
        placement.requested_cells,
        cell,
    );
    // A partial bottom row still intersects, including its physical within-cell offset.
    ((size[1] + f64::from(placement.offset[1])) / f64::from(cell[1])).ceil() as usize
}

/// Copies placement values and validates the immutable pixels at the neutral seam.
fn seal(
    placement: &crate::images::Placement,
    image: &crate::images::Image,
    column: usize,
    row: i64,
) -> Option<Placement> {
    // Defend rendering even if a custom platform decoder returned incomplete pixels.
    let source = ImageSource::new(image.pixels.size, image.pixels.rgba.clone())?;
    // Only the Arc handle is cloned; no pixel or grid-row allocation occurs here.
    Some(Placement {
        source,
        id: image.id,
        generation: image.generation,
        anchor: [column as i64, row],
        crop: placement.source,
        offset: placement.offset,
        cells: placement.requested_cells,
        z: placement.z,
        tile: None,
        clip_rows: None,
    })
}

/// Prototype resolution visits only rows containing the dedicated scalar.
impl FrameSnapshot {
    /// Seals actual text cells, so deleting a prototype later cannot alter this frame.
    fn capture_placeholders(
        &mut self,
        store: &Store,
        term: &Terminal,
        source_row: &impl Fn(usize) -> Option<usize>,
    ) {
        // Ordinary images use tracked anchors above and never require a cell walk.
        // A transmit-only source has no prototype and cannot make a placeholder visible.
        if !store.placements().iter().any(|p| p.virtual_) {
            return;
        }
        // The extra visible edge row follows the same viewport range as copied text.
        // Historical rows use their retained ring positions, not numeric LineId ordering.
        for row in 0..self.rows() {
            let Some(line) = source_row(row).and_then(|index| term.screen().ring.retained(index))
            else {
                continue;
            };
            // visit checks the VT summary before touching any cells on an ordinary row.
            // Resolution happens while the session still exclusively owns the store.
            crate::images::placeholders::visit(line, |column, tile| {
                // Unknown IDs are blank cells, not missing font glyph requests.
                let Some((prototype, image)) = store.virtual_placement(tile) else {
                    return;
                };
                // Prototype extents bound source coordinates even for hostile marks.
                if tile.position[0] >= prototype.cells[1] || tile.position[1] >= prototype.cells[0]
                {
                    return;
                }
                // Actual text location replaces the prototype's unused tracked position.
                // Only the Arc handle crosses threads; decoded bytes are never copied here.
                if let Some(mut sealed) = seal(prototype, image, column, row as i64) {
                    // Keep source coordinates separate from the destination text column,
                    // allowing editing/reflow to move a tile without changing its UV crop.
                    sealed.tile = Some(tile.position);
                    self.images
                        .get_or_insert_with(Default::default)
                        .placements
                        .push(sealed);
                }
            });
        }
    }
}

/// Folded physical images keep one sampling transform per contiguous source run.
mod projection;
