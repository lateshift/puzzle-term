//! Page budgets preserve the existing native/web format shares (§10.3).
//!
//! Pages allocate lazily. Complete native frames may recycle them through the
//! existing ordered flush path; reducing the allowance never drops covered ink.
//! A reload retires old addresses only between builds, not during preparation.
use super::{COLOR_PAGE_BYTES, MASK_PAGE_BYTES, MIN_BUDGET_BYTES, RasterAtlas};

impl RasterAtlas {
    /// Constructs a byte-bounded atlas, preserving native 8:1 and web 4:2 defaults.
    pub fn with_budget(bytes: usize, synchronous: bool) -> Self {
        let bytes = bytes.max(MIN_BUDGET_BYTES);
        // Keep the established color share; then spend remaining bytes on masks.
        // The instance ABI has eight layer bits, so larger settings cannot exceed
        // 256 layers of either kind even when a device supports more.
        let default_colors = if synchronous { 1 } else { 2 };
        let colors = (bytes / puzzle_slug::atlas::DEFAULT_BUDGET_BYTES * default_colors
            + bytes % puzzle_slug::atlas::DEFAULT_BUDGET_BYTES * default_colors
                / puzzle_slug::atlas::DEFAULT_BUDGET_BYTES)
            .clamp(1, 256);
        let masks = ((bytes - colors * COLOR_PAGE_BYTES) / MASK_PAGE_BYTES).clamp(1, 256);
        Self {
            limits: [masks, colors],
            budget_bytes: bytes,
            synchronous,
            ..Self::default()
        }
    }

    /// Retires cache addresses while retaining the installed device-bound producer.
    pub fn replace_budget(&mut self, bytes: usize) {
        let mut replacement = Self::with_budget(bytes, self.synchronous);
        // Scratch is reproducible and independent of permanent sampled pages.
        // Keep its GPU producer; old glyph-to-page identities cannot survive.
        replacement.grid = std::mem::take(&mut self.grid);
        replacement.grid.outlines.clear();
        *self = replacement;
    }

    /// Returns the allowance, including the unused space between whole pages.
    pub fn budget_bytes(&self) -> usize {
        self.budget_bytes
    }

    /// Reports fixed page limits for tests and the platform's GPU owner.
    pub fn page_limits(&self) -> [usize; 2] {
        self.limits
    }
}

#[cfg(test)]
/// Whole-page eviction and format-share regressions.
mod tests;
