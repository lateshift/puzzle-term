//! Dev-only coverage-guided harness shared by M19.2b's isolated targets.
//! Production services are replaced only at the documented host seams; the VT,
//! Session, registered Rex handler and graphics state are the real code paths.
//! Fresh bounded owners make every finding reproducible from one input file.
/// No-OS Session construction and bounded native-decode completion.
mod session;
/// Structured OSC 66 operations keep edits and resizes reachable by mutation.
mod sizing;
pub use session::{drive, new_session};
pub use sizing::text_sizing;

/// Small fixture history reaches eviction quickly without changing product defaults.
pub(crate) const HISTORY_LINES: usize = 64;
/// Independent byte cap bounds rare cluster/block payloads as well as row count.
pub(crate) const HISTORY_BYTES: usize = 64 * 1024;

/// Runs the expensive ownership oracle after every input operation/chunk.
pub fn invariants(term: &puzzle_term::vt::Terminal) {
    // cargo-fuzz is always invoked with debug assertions: the oracle must not
    // silently disappear merely because fuzz binaries use optimized code.
    #[cfg(not(debug_assertions))]
    compile_error!("Puzzle fuzz targets require --debug-assertions");
    #[cfg(debug_assertions)]
    for screen in [&term.primary, &term.alternate] {
        // Both stream and structured sizing drivers call this shared boundary.
        // Ordinary wide/cluster defects must be caught before block accounting.
        if let Err(error) = screen.ring.check_grid() {
            panic!("grid invariant: {error}");
        }
        if let Err(error) = screen.ring.check_multicells() {
            panic!("multicell invariant: {error}");
        }
    }
    let (columns, rows) = term.size();
    assert!(term.cursor.row < rows && term.cursor.column < columns);
    // A pure terminal cannot grow history beyond the explicitly tiny fixture.
    assert!(term.primary.ring.scrollback_len() <= HISTORY_LINES);
}
