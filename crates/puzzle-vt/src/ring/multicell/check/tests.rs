//! Full validation must not scan retained history once per rectangular fragment.
use crate::{Terminal, ring::index_operations};

#[test]
fn checker_uses_no_product_line_lookups_over_sparse_history() {
    let mut term = Terminal::new(40, 10, 64, 1024 * 1024);
    for _ in 0..80 {
        term.advance(b"\x1b]66;s=2:w=2;ab\x1b\\\r\n");
    }
    // Region edits make IDs sparse/reordered, so index_of could not rely only
    // on its guessed contiguous slot. The oracle instead uses its one map.
    term.advance(b"\x1b[2;7r\x1b[7;1H\n\x1b[r");
    // An empty sparse model would make the zero-lookup proof vacuous.
    assert!(term.screen().ring.iter().any(|row| !row.blocks.is_empty()));
    index_operations::reset();
    assert!(term.screen().ring.check_multicells().is_ok());
    assert_eq!(index_operations::read(), 0);
}
