//! D-47 batches cold grid coverage while preserving queue-ordered page reuse.
//!
//! GPU-generated masks are a disposable cache of reproducible vector sources.
//! Only neutral device-local tickets cross the frame contract. Coverage remains
//! on the GPU: no readback, CPU page mirror or duplicated bitmap bytes exist.
//! Authored image-only glyphs retain their established restoration mirrors.
//! Cold batches end before scratch reuse, page eviction or upload collection.
//! Each ticket names an R8 crop whose source texture lives until acknowledgement.
//! Queue order captures used pages before clears, so earlier draw segments keep ink.
//! Device loss discards grid handles and pending old-device tickets; fonts supply
//! fresh coverage on demand. Warm hits perform no rasterization or allocation.
//! CPU-only hosts use the same Slug oracle, with complete crops before insertion.
//! Hinted/unhinted outline selection and cell placement stay upstream and unchanged.
//! Scratch vectors occupy one bounded five-MiB page pair, reset after each batch.
//! This fits alongside the live vector atlas's forty-MiB maximum without relaxing
//! its forty-eight-MiB allowance. Temporary R8 sheets are upload staging ownership,
//! never additional permanent sampled cache layers. Bold uses the same integer
//! source-over offset as the vector emitter. Effects reject outline-derived masks;
//! image-only emoji retain their separate effective-size transform path.
//! CPU vector-overflow samples also carry their reproducible source identity.
//! Losing those handles triggers outline preparation rather than a stale mirror
//! upload. Authored samples sharing their page retain only their own pixel data.
use super::{BitmapKind, PAGE, RasterAtlas, RasterHandle, RasterKey, RasterUpload};
use crate::{
    glyph::{GlyphPlacement, oracle::OracleRasterizer},
    preparation::Outline,
};
use puzzle_slug::{ShapeHandle, ShapeKey, SlugAtlas, atlas::FillRule};
/// CPU oracle and vector-overflow crops have the same reproducible ownership.
mod cpu;

/// One white Slug coverage draw; the platform adapter supplies the GPU instance ABI.
#[derive(Clone, Copy)]
pub struct MaskDraw {
    /// Current packed scratch shape, valid until this batch completes.
    pub shape: ShapeHandle,
    /// Em-to-device linear map, in column-major order.
    pub matrix: [f32; 4],
    /// Device baseline inside the transient coverage sheet.
    pub translation: [f32; 2],
}

/// Optional synchronous GPU producer, called only by the frame-building owner.
pub trait GridRasterizer: Send {
    /// Renders white coverage with the existing Slug shader and returns a device-local ticket.
    /// `size` bounds the scratch sheet; errors leave pending writes unacknowledged.
    fn bake(
        &mut self,
        atlas: &mut SlugAtlas,
        draws: &[MaskDraw],
        size: [u32; 2],
    ) -> Result<(u64, std::sync::Arc<()>), String>;
}

/// A reserved shelf write awaits one cold GPU batch, never another presented frame.
struct Pending {
    /// Existing dirty entry receives its GPU crop ticket in original queue order.
    upload: usize,
    /// Top-left coordinate of this glyph's coverage crop in the scratch sheet.
    at: [u32; 2],
}

/// Scratch data is bounded by one Slug page pair and one 2048² coverage sheet.
#[derive(Default)]
pub(super) struct Cache {
    /// CPU fallback avoids a GPU requirement in neutral and headless text clients.
    oracle: Option<OracleRasterizer>,
    /// Host-installed producer owns GPU objects without exposing them to layout.
    gpu: Option<Box<dyn GridRasterizer>>,
    /// Packed scratch is lazy and reset after every bounded batch.
    atlas: Option<SlugAtlas>,
    /// Draw records retain no font paths or source bytes after packing.
    draws: Vec<MaskDraw>,
    /// Upload addresses are collected only until this batch completes synchronously.
    pending: Vec<Pending>,
    /// Scratch shelf cursor and maximum height; transparent gutters separate dilation.
    shelf: [u32; 3],
    /// Which ordinary raster keys came from outlines, for later vector effects.
    pub(super) outlines: std::collections::HashSet<RasterKey>,
}

impl RasterAtlas {
    /// Installs the device-bound producer after the host invalidates old-device grid handles.
    pub fn set_grid_rasterizer(&mut self, gpu: Box<dyn GridRasterizer>) -> Result<(), String> {
        // A source-policy replacement has no live pending build; lost-device tickets are obsolete.
        self.grid.gpu = Some(gpu);
        Ok(())
    }
    /// Effect requests may reuse image-only sources, but never a grid outline mask.
    pub fn get_for_placement(&mut self, key: RasterKey, effect: bool) -> Option<RasterHandle> {
        // Image-only masks have no corresponding vector, so their established path survives.
        if effect && self.grid.outlines.contains(&key) {
            return None;
        }
        self.get(key)
    }
    /// Publishes prepared geometry as a size-specific Slug mask without a vector duplicate.
    pub fn prepare_grid(
        &mut self,
        key: RasterKey,
        p: &GlyphPlacement,
        ink: Outline,
        style: [bool; 3],
    ) -> Result<RasterHandle, String> {
        // A warm handle needs neither its vector source nor a GPU batch.
        if let Some(handle) = self.get(key) {
            return Ok(handle);
        }
        // No GPU installation is required for neutral/headless text clients.
        if self.grid.gpu.is_none() {
            // CPU-only callers still consume the converted curves and bands just once.
            let pixels = self
                .grid
                .oracle
                .get_or_insert_with(Default::default)
                .grid(p, ink, style)?;
            // Oracle coverage is reproducible, just like masks made by the native GPU.
            return self.prepare_grid_mask(key, pixels);
        }
        // The GPU path reserves only shelf metadata, then transfers a complete batch ticket.
        self.queue_grid(key, p, ink, style)
    }
    /// Packs cold GPU work; crossing either scratch bound flushes before reuse.
    fn queue_grid(
        &mut self,
        key: RasterKey,
        p: &GlyphPlacement,
        ink: Outline,
        style: [bool; 3],
    ) -> Result<RasterHandle, String> {
        // The crop uses the identical baseline-relative fit as oracle/vector emission.
        let grid = crate::glyph::oracle::geometry::SampleGrid::grid(p, ink.shape.bbox, style)?;
        // A sheet boundary completes its earlier batch before new addresses are assigned.
        let at = self.grid_space(grid.width, grid.height)?;
        // reserve() can flush older pending writes before it recycles a real raster page.
        let handle = self.reserve_grid(key, grid.width, grid.height, grid.offset, 0)?;
        // prepare appended exactly this reservation; completion patches that ordered write.
        let upload = self.dirty.len() - 1;
        // Geometry packing can flush earlier draws before resetting local Slug addresses.
        let shape = self.grid_shape(ink)?;
        // A capacity-triggered flush reset the scratch shelf, so reserve it after packing too.
        let at = if self.grid.shelf == [0; 3] {
            self.grid_space(grid.width, grid.height)?
        } else {
            at
        };
        // Capture final coordinates only after every earlier batch has completed.
        self.grid_draw(upload, at, grid, style[2], shape);
        // A later transformed request recognizes this source as reproducible vector ink.
        self.grid.outlines.insert(key);
        Ok(handle)
    }
    /// Appends coverage transforms after all capacity-triggered flushes have assigned addresses.
    fn grid_draw(
        &mut self,
        upload: usize,
        at: [u32; 2],
        grid: crate::glyph::oracle::geometry::SampleGrid,
        bold: bool,
        shape: ShapeHandle,
    ) {
        // Removing the crop offset places the em origin inside its assigned sheet rectangle.
        let translation = [
            at[0] as f32 - grid.offset[0] as f32 + grid.translation[0],
            at[1] as f32 - grid.offset[1] as f32 + grid.translation[1],
        ];
        // Missing italic was rejected by policy; this matrix is an untransformed grid fit.
        let first = MaskDraw {
            shape,
            matrix: [grid.size, 0.0, 0.0, -grid.size],
            translation,
        };
        // These instances are ordered exactly as vector synthesis would draw them.
        self.grid.draws.push(first);
        if bold {
            // A second integer-offset source-over draw matches synthetic bold vectors.
            self.grid.draws.push(MaskDraw {
                translation: [translation[0] + grid.bold, translation[1]],
                ..first
            });
        }
        // The returned handle cannot leave upload collection before this reservation is filled.
        self.grid.pending.push(Pending { upload, at });
    }
    /// Submits every queued crop before uploads or eviction can select its ticket.
    pub fn flush_grid(&mut self) -> Result<(), String> {
        // This early return is the warm-frame allocation and GPU-work guarantee.
        if self.grid.pending.is_empty() {
            return Ok(());
        }
        // Only occupied rows become an R8 staging target; unused sheet height costs no bytes.
        let size = [PAGE, self.grid.shelf[1] + self.grid.shelf[2]];
        let atlas = self
            .grid
            .atlas
            .as_mut()
            .ok_or("grid scratch atlas absent")?;
        // The synchronous seam submits coverage work; queue order supplies completion before copies.
        let (ticket, lease) = self
            .grid
            .gpu
            .as_mut()
            .ok_or("grid GPU producer absent")?
            .bake(atlas, &self.grid.draws, size)?;
        // Attach GPU crops to their existing ordered writes; no pixel bytes cross this seam.
        for pending in &self.grid.pending {
            let upload = &mut self.dirty[pending.upload];
            upload.gpu = Some((ticket, pending.at, std::sync::Arc::clone(&lease)));
        }
        // The permanent owner is now only coverage; scratch vectors do not enter the live cache.
        self.grid.pending.clear();
        self.grid.draws.clear();
        self.grid.shelf = [0; 3];
        // Packing reuses its allocations, while the submitted GPU command keeps old texels alive.
        atlas.reset().map_err(|e| e.to_string())?;
        Ok(())
    }
    /// Assigns transient sheet space with a gutter before analytic dilation reaches neighbors.
    fn grid_space(&mut self, width: u32, height: u32) -> Result<[u32; 2], String> {
        // Scratch packing never alters final shelf UVs or glyph origins.
        let [mut x, mut y, mut row] = self.grid.shelf;
        // The gutter is included in fit checks so analytic dilation never crosses another crop.
        if x + width + 2 > PAGE {
            x = 0;
            y += row;
            row = 0;
        }
        // A full sheet submits the previous batch before restarting its local coordinate system.
        if y + height + 2 > PAGE {
            self.flush_grid()?;
            x = 0;
            y = 0;
            row = 0;
        }
        // Remember the highest crop in this scratch shelf before the next wrap.
        self.grid.shelf = [x + width + 2, y, row.max(height + 2)];
        Ok([x + 1, y + 1])
    }
    /// Retries a scratch page only after its recorded draws have completed.
    fn grid_shape(&mut self, ink: Outline) -> Result<ShapeHandle, String> {
        // A Slug-only context pays for no scratch textures or geometry mirrors.
        if self.grid.atlas.is_none() {
            self.grid.atlas =
                Some(SlugAtlas::with_budget(5 * 1024 * 1024).map_err(|e| e.to_string())?);
        }
        let atlas = self.grid.atlas.as_mut().ok_or("grid scratch absent")?;
        // Pending count gives collision-free identities within this scratch batch.
        let id = self.grid.pending.len() as u64 + 1;
        // Keep retry input only across this bounded insertion; there is no retained outline cache.
        match atlas.prepare_geometry(ShapeKey(id), &ink.shape, &ink.bands, FillRule::NonZero, 0) {
            Ok(handle) => Ok(handle),
            Err(puzzle_slug::atlas::AtlasError::Capacity) => {
                // Submitted draws own their old packed data; only now may the scratch reset.
                self.flush_grid()?;
                let atlas = self
                    .grid
                    .atlas
                    .as_mut()
                    .ok_or("grid scratch absent after flush")?;
                atlas
                    .prepare_geometry(ShapeKey(1), &ink.shape, &ink.bands, FillRule::NonZero, 0)
                    .map_err(|e| e.to_string())
            }
            // Malformed geometry remains an error; capacity alone is handled by flushing.
            Err(error) => Err(error.to_string()),
        }
    }
}

/// Reproducible cache presence affects cold page ownership without exposing native types.
impl Cache {
    /// Both grid producers can regenerate coverage from fonts instead of keeping source pixels.
    pub(super) fn reproducible_enabled(&self) -> bool {
        self.gpu.is_some() || self.oracle.is_some()
    }
}

impl RasterAtlas {
    /// Reserves reproducible grid metadata and an ordered crop placeholder without a CPU mirror.
    fn reserve_grid(
        &mut self,
        key: RasterKey,
        width: u32,
        height: u32,
        offset: [i32; 2],
        gutter: u32,
    ) -> Result<RasterHandle, String> {
        // Reservation bounds match authored bitmap insertion before any shelf changes.
        if width == 0 || height == 0 || width > PAGE - 2 || height > PAGE - 2 {
            return Err("grid crop exceeds raster page".into());
        }
        // Bounded identities retire with their LRU page, rather than growing without limit.
        if self.entries.len() >= 65536 {
            self.reclaim(BitmapKind::Mask)?;
        }
        // Grid masks are rejected by effect lookup, so only exact nearest sampling
        // reaches these packed crops. Scratch retains its analytic dilation gutter.
        // reserve may capture and recycle a page; no previous UV is changed in place.
        let (slot, rect) = self.reserve(BitmapKind::Mask, width, height, gutter)?;
        // Logical page indices count masks only, independently of authored color layers.
        let page = self.page_index(slot);
        self.pages[slot].used = self.frame;
        // Mixed authored/grid pages also require GPU snapshots to preserve their generated ink.
        self.pages[slot].gpu = true;
        // Epoch selects the correct incarnation if this same physical layer is reused again.
        let handle = RasterHandle {
            generation: self.generation,
            page,
            epoch: self.pages[slot].epoch,
            rect,
            kind: BitmapKind::Mask,
            offset,
        };
        // flush_grid fills this ticket before upload ownership leaves the text atlas.
        self.dirty.push(RasterUpload {
            kind: BitmapKind::Mask,
            page,
            rect,
            pixels: Vec::new(),
            gpu: None,
            capture: None,
        });
        // Metadata publication cannot let bytes leave before flush_grid fills the ticket.
        self.entries.insert(key, handle);
        Ok(handle)
    }
    /// Drops reproducible GPU ink after loss, retaining only image-only restoration sources.
    pub(super) fn invalidate_gpu_grid(&mut self) {
        // Authored-only caches own complete bytes and need no grid invalidation.
        if self.grid.outlines.is_empty() && self.grid.pending.is_empty() {
            return;
        }
        // Pending tickets belong to the lost device and must never be flushed through it.
        self.grid.pending.clear();
        self.grid.draws.clear();
        self.grid.shelf = [0; 3];
        // Losing this bounded scratch owner also discards unsubmitted old-device geometry.
        self.grid.atlas = None;
        self.versions.clear();
        self.retired.clear();
        // Authored image keys remain valid; their mirrors will be uploaded to the new device.
        self.entries
            .retain(|key, _| !self.grid.outlines.contains(key));
        self.grid.outlines.clear();
        // Pure GPU shelves can restart immediately. Mixed authored shelves keep their addresses.
        for page in &mut self.pages {
            page.gpu = false;
            // A pure disposable page has no authored UVs that must survive its shelf reset.
            if page.pixels.is_empty() {
                page.x = 0;
                page.y = 0;
                page.height = 0;
                page.shelves.clear();
                page.epoch = page.epoch.wrapping_add(1);
            }
        }
        // Every session rejects stale row handles before on-demand rasterization resumes.
        self.generation = self.generation.wrapping_add(1);
    }
}
