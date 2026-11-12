//! Shared ordinary-grid assertions for VT functional tests and properties.
//! Call explicitly at stable operation boundaries, rather than instrumenting
//! Terminal::advance: allocation and throughput fixtures must keep measuring
//! the real feed path, and temporary internal edits may be inconsistent.
//! Release tests retain their existing behavioral assertions without the oracle.

/// Checks retained history and the inactive screen as well as the current grid.
pub(crate) fn check(term: &crate::Terminal) {
    #[cfg(debug_assertions)]
    for screen in [&term.primary, &term.alternate] {
        assert_eq!(screen.ring.check_grid(), Ok(()));
    }
    // A release-profile functional test has no debug-only grid API to call.
    #[cfg(not(debug_assertions))]
    let _ = term;
}
