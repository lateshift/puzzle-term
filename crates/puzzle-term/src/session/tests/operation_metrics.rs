//! Coarse Session observations preserve ordered parsing and allocate no warm samples.
use super::*;

/// The observer must not change replies, modes, text or the injected parse budget.
#[test]
fn observed_feed_preserves_cells_replies_and_warm_allocations() {
    let (mut observed, observed_writes, _) = fixture();
    let (mut ordinary, ordinary_writes, _) = fixture();
    let bytes = "abcd\r\n一\u{301}\x1b[2K\x1b[6n".as_bytes();
    observed.start_parse_operation_metrics();
    let now = Instant::now();
    let first = observed.feed(bytes, now);
    let second = ordinary.feed(bytes, now);
    assert_eq!(first.parsed_bytes, second.parsed_bytes);
    assert_eq!(first.pending_bytes, second.pending_bytes);
    assert_eq!(
        *observed_writes.lock().expect("writes"),
        *ordinary_writes.lock().expect("writes")
    );
    for row in 0..observed.terminal.size().1 {
        assert_eq!(
            observed.terminal.screen().row(row).map(|r| &r.cells),
            ordinary.terminal.screen().row(row).map(|r| &r.cells)
        );
    }
    assert!(observed.parse_operation_metrics()[4] > 0);
    assert!(observed.parse_feed_phases()[1] > 0);
    assert_eq!(ordinary.parse_feed_phases(), [0; 6]);
    // Reply recording intentionally allocates payloads, so the warmed sample has no query.
    // Both scalar copy storage and observations are already initialized above.
    observed.feed(b"x", Instant::now());
    let allocations = allocation_counter::measure(|| {
        observed.start_parse_operation_metrics();
        observed.feed(b"x", Instant::now());
        std::hint::black_box(observed.parse_feed_phases());
    });
    assert_eq!(allocations.count_total, 0);
}
