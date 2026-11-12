//! Dirty upload union preserves exact written addresses despite insertion order.
use super::{DirtySpan, TexelStore};
use proptest::prelude::*;

proptest! {
    /// Late header writes and revisited rows must neither lose texels nor upload gaps.
    #[test]
    fn publication_preserves_the_exact_dirty_union(spans in prop::collection::vec((0_u16..4, 0_u16..4000, 1_u16..96), 1..100)) {
        let mut store = TexelStore::<[u16; 2]>::new();
        let mut written = vec![false; 4 * 4096];
        for (row, start, length) in spans {
            let end = start + length;
            store.dirty.push(DirtySpan { row, start, end });
            written[usize::from(row) * 4096 + usize::from(start)..usize::from(row) * 4096 + usize::from(end)].fill(true);
        }
        store.coalesce_dirty();
        let mut uploaded = vec![false; written.len()];
        for span in &store.dirty {
            uploaded[usize::from(span.row) * 4096 + usize::from(span.start)..usize::from(span.row) * 4096 + usize::from(span.end)].fill(true);
        }
        prop_assert_eq!(uploaded, written);
        // Touching intervals have exactly one upload; a hole keeps separate writes.
        prop_assert!(store.dirty.windows(2).all(|pair| pair[0].row < pair[1].row || pair[0].end < pair[1].start));
    }
}
