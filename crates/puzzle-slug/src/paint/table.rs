//! CPU paint rows with bounded allocation and generational eviction (§9.8).
//!
//! Row zero is reserved for the M3 binding probe. Records keep their CPU
//! bytes after upload, so device restoration does not traverse fonts again.
//!
//! The byte allowance includes reserved GPU rows, not just occupied entries.
//! Each row is 256 RGBA32Float texels, or 4096 bytes. Capacity starts small
//! and doubles until reaching that allowance. Row zero is never allocated
//! to a color glyph, protecting the older solid-paint binding probe.
//! Callers supply stable identities and frame numbers. A hit refreshes last
//! use, while a full table compacts records used within the preceding 600
//! frames. All previous handles are invalid after any such compaction.
//! Dirty rows contain owned copies because a hidden frame may outlive another
//! atlas modification. The retained Entry row remains the restoration source.
//! Effect rows retain every geometric texel from the source row.
//! Clip transforms and curve addresses therefore survive recoloring verbatim.
//! Foreground sentinels are resolved with the requesting instance's straight RGBA.
//! Resolving a sentinel first avoids multiplying replacement RGB by old RGB.
//! Authored colors are already premultiplied and keep their palette alpha.
//! An explicit override replaces RGB while multiplying its own alpha into the layer.
//! Opacity scales all four channels, keeping the blend equation premultiplied.
//! Gradient positions, offsets and extend mode are independent of that color operation.
//! A variant is a separate row so another Session can retain the original paint.
//! Live insertion grows the texture without moving any existing row address.
//! When the allowance is exhausted, insertion returns an error to the frame builder.
//! Only begin_frame may compact those addresses before row caches are captured.
//! Source rows must resolve in the current incarnation before a variant is copied.
//! The common identity allocator prevents effects from aliasing ordinary COLR paints.
//! Dirty copies are owned by the pending frame after collection.
//! A skipped presentation therefore cannot acknowledge or lose a new variant.
//! Restore includes variant rows in the same mirror as ordinary authored paints.
//! The zero row remains reserved even when the first insertion is an effect.
//! The default table bounds both variant and source rows together.
//!
//! Boundary eviction keeps the preceding frame's paint rows alive during the next build.
//! Paint address changes invalidate generation-checked rows before collecting instances.

use super::Paint;
use std::collections::BTreeMap;
/// A row address and incarnation, checked before cached layers are reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaintHandle {
    /// Changes whenever rows are compacted or reset.
    pub generation: u64,
    /// Linear first-texel index plus one, matching SlugInstance.paint.
    pub index: u32,
}
/// One retained paint record, its address and recent-use timestamp.
struct Entry {
    /// Encoded CPU source of truth, including enclosing clip handles.
    row: [[f32; 4]; 256],
    /// Current shader-visible row index.
    handle: PaintHandle,
    /// Frame number of last explicit use.
    used: u64,
}
/// Paint mirror bounded by a caller-selected byte allowance.
pub struct PaintTable {
    /// Stable caller identities resolve to movable row handles.
    entries: BTreeMap<u64, Entry>,
    /// Pending full-row copies; taking them transfers ownership to a frame.
    dirty: Vec<(u32, Vec<[f32; 4]>)>,
    /// Current incarnation; stale cached instances fail validation.
    generation: u64,
    /// Complete allocated row capacity, including the reserved first row.
    capacity: usize,
    /// Hard maximum rows, derived from bytes / 4096.
    limit: usize,
}
impl Default for PaintTable {
    /// Uses a one-MiB allowance, accommodating 255 active paint records.
    fn default() -> Self {
        Self::new(1024 * 1024)
    }
}
impl PaintTable {
    /// Starts at two rows and grows geometrically under the byte allowance.
    pub fn new(budget: usize) -> Self {
        // Reserve only a power-of-two height that the GPU growth policy can match.
        // One usable row follows the probe; larger limits never exceed the allowance.
        let rows = (budget / 4096).clamp(2, 8192);
        Self {
            entries: BTreeMap::new(),
            dirty: Vec::new(),
            generation: 1,
            capacity: 2,
            limit: 1usize << rows.ilog2(),
        }
    }
    /// Returns and touches a row only when the complete handle is current.
    pub fn get(&mut self, key: u64, frame: u64) -> Option<PaintHandle> {
        // Touch only existing records; lookup never allocates missing entries.
        let entry = self.entries.get_mut(&key)?;
        entry.used = frame;
        Some(entry.handle)
    }
    /// Tests whether a cached layer still names this mirror's incarnation.
    pub fn contains(&self, handle: PaintHandle) -> bool {
        handle.generation == self.generation
    }
    /// Advances a live frame before snapshot capture; moved rows invalidate caches.
    /// Assembly may already have copied addresses into unchanged row slices.
    /// Relocating while adding another glyph would corrupt those earlier slices.
    /// This pre-capture boundary lets the host advance its row-cache epoch.
    pub fn begin_frame(&mut self, frame: u64) -> Result<bool, String> {
        // Eviction happens before any current-frame instance captures an address.
        let stale = self
            .entries
            .values()
            .any(|e| frame.saturating_sub(e.used) > 600);
        if stale && self.entries.len() + 1 >= self.capacity {
            self.compact(frame)?;
            return Ok(true);
        }
        Ok(false)
    }
    /// Refreshes usage from assembled row instances, including unchanged rows.
    pub fn mark_index(&mut self, index: u32, frame: u64) {
        // A paint row is bounded to 255 records by the default allowance.
        if let Some(entry) = self.entries.values_mut().find(|e| e.handle.index == index) {
            entry.used = frame;
        }
    }
    /// Exposes outer clip dependencies when a cold row registers its shape batch.
    /// The encoded row is the CPU source of truth for addresses sampled by the shader.
    pub fn clip_locations(&self, index: u32) -> impl Iterator<Item = u32> + '_ {
        self.entries
            .values()
            .find(|entry| entry.handle.index == index)
            .into_iter()
            .flat_map(|entry| {
                (0..entry.row[0][3] as usize).map(|i| {
                    // Paint encoding writes exact u16 coordinates as f32; these
                    // values are lossless before packing the shared instance word.
                    let loc = entry.row[41 + i * 2];
                    crate::encoding::encode_glyph_loc([loc[0] as u16, loc[1] as u16])
                })
            })
    }
    /// Allocates a standalone record, compacting cold entries at capacity.
    pub fn prepare(&mut self, key: u64, paint: &Paint, frame: u64) -> Result<PaintHandle, String> {
        self.insert(key, paint, frame, true)
    }
    /// Inserts during frame assembly without moving addresses already captured.
    /// Growth copies existing rows without changing their indices.
    /// Capacity exhaustion returns a report instead of relocating live addresses.
    pub fn prepare_live(
        &mut self,
        key: u64,
        paint: &Paint,
        frame: u64,
    ) -> Result<PaintHandle, String> {
        self.insert(key, paint, frame, false)
    }
    /// Shares validation and growth while making the relocation boundary explicit.
    fn insert(
        &mut self,
        key: u64,
        paint: &Paint,
        frame: u64,
        compact: bool,
    ) -> Result<PaintHandle, String> {
        if let Some(handle) = self.get(key, frame) {
            return Ok(handle);
        }
        // A malformed brush must fail before eviction changes another record.
        let row = paint.texels()?;
        self.insert_row(key, row, frame, compact)
    }
    /// Allocates an already validated record without relocating live frame addresses.
    fn insert_row(
        &mut self,
        key: u64,
        row: [[f32; 4]; 256],
        frame: u64,
        compact: bool,
    ) -> Result<PaintHandle, String> {
        if self.entries.len() + 1 >= self.capacity {
            if self.capacity < self.limit {
                self.capacity = (self.capacity * 2).min(self.limit);
            } else if compact {
                self.compact(frame)?;
            }
        }
        if self.entries.len() + 1 >= self.capacity {
            return Err("active paint rows exceed memory allowance".into());
        }
        // Row-major first texel plus one is the SlugInstance nonzero convention.
        let index = ((self.entries.len() + 1) * 256 + 1) as u32;
        let handle = PaintHandle {
            generation: self.generation,
            index,
        };
        // Taking dirty copies is safe only because the CPU row stays retained.
        self.dirty.push((index - 1, row.to_vec()));
        self.entries.insert(
            key,
            Entry {
                row,
                handle,
                used: frame,
            },
        );
        Ok(handle)
    }
    /// Copies one validated source row with resolved foreground and ink-only color/alpha.
    /// Geometry, stops and enclosing clip addresses remain byte-identical.
    pub fn prepare_effect(
        &mut self,
        key: u64,
        source: u32,
        foreground: u32,
        color: Option<u32>,
        opacity: f32,
        frame: u64,
    ) -> Result<PaintHandle, String> {
        if let Some(hit) = self.get(key, frame) {
            return Ok(hit);
        }
        // Source addresses are only read during this frame's valid atlas incarnation.
        let mut row = self
            .entries
            .values()
            .find(|e| e.handle.index == source)
            .ok_or("effect source paint expired")?
            .row;
        if row[0][0] == -1.0 {
            row[5] = effect_color(row[5], row[6][0] != 0.0, foreground, color, opacity);
            // Opacity-only variants retain foreground sentinels for selection/cursor.
            if color.is_some() {
                row[6][0] = 0.0;
            }
        }
        for i in 0..row[0][2] as usize {
            // Resolve dynamic sentinel colors before baking alpha/replacement RGB.
            row[9 + i * 2] = effect_color(
                row[9 + i * 2],
                row[8 + i * 2][1] != 0.0,
                foreground,
                color,
                opacity,
            );
            if color.is_some() {
                row[8 + i * 2][1] = 0.0;
            }
        }
        self.insert_row(key, row, frame, false)
    }

    /// Moves pending rows into a frame without discarding the source mirror.
    pub fn take_dirty(&mut self) -> Vec<(u32, Vec<[f32; 4]>)> {
        std::mem::take(&mut self.dirty)
    }
    /// Marks all occupied rows for upload to a newly created GPU texture.
    pub fn restore(&mut self) {
        self.dirty.clear();
        for entry in self.entries.values() {
            self.dirty
                .push((entry.handle.index - 1, entry.row.to_vec()));
        }
    }
    /// Reports the actual reserved GPU-equivalent bytes for measurement.
    pub fn allocated_bytes(&self) -> usize {
        self.capacity * 4096
    }
    /// Reassigns surviving rows and invalidates every old shader address.
    fn compact(&mut self, frame: u64) -> Result<(), String> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("paint generation exhausted")?;
        // Retention follows actual submitted instance usage, not font discovery.
        self.entries
            .retain(|_, entry| frame.saturating_sub(entry.used) <= 600);
        // Compact addresses densely and stamp every survivor with the new incarnation.
        for (i, entry) in self.entries.values_mut().enumerate() {
            entry.handle = PaintHandle {
                generation: self.generation,
                index: ((i + 1) * 256 + 1) as u32,
            };
        }
        // Every moved survivor is uploaded before rebuilt row instances can draw.
        self.restore();
        Ok(())
    }
}

/// Changes premultiplied color after resolving any instance-foreground sentinel.
fn effect_color(
    mut rgba: [f32; 4],
    sentinel: bool,
    foreground: u32,
    color: Option<u32>,
    opacity: f32,
) -> [f32; 4] {
    // Dynamic foreground alpha comes from the already opacity-adjusted instance.
    // Leave its stop alpha intact to avoid applying the effect twice.
    if sentinel && color.is_none() {
        return rgba;
    }
    if sentinel {
        let fg = foreground.to_le_bytes().map(|v| f32::from(v) / 255.0);
        // Sentinel paint alpha multiplies the instance alpha before applying the effect.
        rgba[3] *= fg[3];
        for i in 0..3 {
            rgba[i] = fg[i] * rgba[3];
        }
    }
    if let Some(color) = color {
        let replacement = color.to_le_bytes().map(|v| f32::from(v) / 255.0);
        // Preserve authored coverage while applying replacement alpha independently.
        rgba[3] *= replacement[3];
        for i in 0..3 {
            rgba[i] = replacement[i] * rgba[3];
        }
    }
    for channel in &mut rgba {
        *channel *= opacity;
    }
    rgba
}
