//! Configured bytes, not the historical default, decide when LRU starts.
use super::*;
use crate::raster::tests::{full, key};

/// Native 24 MiB admits two mask pages and a color page, then recycles the oldest.
#[test]
fn nondefault_budget_evicts_at_configured_capacity() {
    let mut atlas = RasterAtlas::with_budget(MIN_BUDGET_BYTES + MASK_PAGE_BYTES, true);
    assert_eq!(atlas.page_limits(), [2, 1]);
    for index in 0..5 {
        atlas.begin_frame();
        assert!(atlas.prepare(key(index), full(index as u8)).is_ok());
        atlas.take_dirty();
        assert!(atlas.allocated_bytes() <= atlas.budget_bytes());
        assert_eq!(atlas.evictions(), u64::from(index.saturating_sub(1)));
    }
    assert!(atlas.get(key(0)).is_none());
    assert!(atlas.get(key(4)).is_some());
    assert_eq!(atlas.allocated_bytes(), MASK_PAGE_BYTES * 2);
}

/// Established format allocations are unchanged at the default allowance.
#[test]
fn default_format_capacities_and_minimum_fit() {
    let bytes = puzzle_slug::atlas::DEFAULT_BUDGET_BYTES;
    assert_eq!(RasterAtlas::with_budget(bytes, false).page_limits(), [4, 2]);
    assert_eq!(RasterAtlas::with_budget(bytes, true).page_limits(), [8, 1]);
    for synchronous in [false, true] {
        let atlas = RasterAtlas::with_budget(MIN_BUDGET_BYTES, synchronous);
        assert_eq!(atlas.page_limits(), [1, 1]);
        assert_eq!(atlas.budget_bytes(), MIN_BUDGET_BYTES);
    }
}
