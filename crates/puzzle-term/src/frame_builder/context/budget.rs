//! Each text context owns separate vector and raster allowances (§4.6).
//! The structural minimum comes from fixed page formats, never a tuned number.
use super::TextContext;
/// Bytes in the user-facing binary megabyte unit.
pub const MIB: usize = 1024 * 1024;
/// Smallest allowance admitting one complete page of every required kind.
pub const MIN_BYTES: usize =
    if puzzle_text::raster::MIN_BUDGET_BYTES > puzzle_slug::atlas::MIN_BUDGET_BYTES {
        puzzle_text::raster::MIN_BUDGET_BYTES
    } else {
        puzzle_slug::atlas::MIN_BUDGET_BYTES
    };
/// Round up to the integer unit accepted by the settings schema.
pub const MIN_MIB: u32 = MIN_BYTES.div_ceil(MIB) as u32;
impl TextContext {
    /// Applies a validated allowance between frames and invalidates shared row caches.
    /// Returns whether the CPU/GPU atlas owners need replacement.
    pub fn set_glyph_memory_mb(&mut self, mib: u32) -> Result<bool, String> {
        // Saturation also protects direct typed callers on 32-bit hosts.
        let bytes = (mib.max(MIN_MIB) as usize).saturating_mul(MIB);
        if bytes == self.glyph_budget_bytes {
            return Ok(false);
        }
        // Native batching owns one scratch vector page. Reserve it within the
        // vector allowance; default live growth still tops out at 40 MiB.
        let scratch = if self.raster.is_synchronous() {
            puzzle_slug::atlas::MIN_BUDGET_BYTES
        } else {
            0
        };
        self.backend.replace_budget(bytes - scratch)?;
        self.raster.replace_budget(bytes);
        self.glyph_budget_bytes = bytes;
        // Every builder sharing this context rejects the retired page addresses.
        self.generation = self.generation.wrapping_add(1);
        Ok(true)
    }
    /// Exposes the selected per-atlas allowance without walking glyph entries.
    pub fn glyph_budget_bytes(&self) -> usize {
        self.glyph_budget_bytes
    }
}
