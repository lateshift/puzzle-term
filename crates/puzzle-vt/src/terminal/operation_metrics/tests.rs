//! The observer describes real semantic work and changes neither cells nor allocations.
use super::*;

/// Bounded source/control handling is counted after parsing, including split sequences.
#[test]
fn real_operations_are_attributed_without_changing_terminal_behavior() {
    let mut t = Terminal::new(4, 2, 10, 4096);
    let mut plain = Terminal::new(4, 2, 10, 4096);
    t.start_operation_metrics();
    let bytes = "abcd\r\nefgh\r\n一\u{301}\x1b[2J\x1b]2;title\x07".as_bytes();
    t.advance(bytes);
    plain.advance(bytes);
    let metrics = t
        .operation_metrics()
        .unwrap_or_else(|| panic!("observer absent"));
    assert_eq!(&metrics[..4], &[1, 0, 2, 8]);
    assert_eq!(&metrics[4..8], &[1, 5, 1, 6]);
    for row in 0..2 {
        assert_eq!(
            t.screen().row(row).map(|r| &r.cells),
            plain.screen().row(row).map(|r| &r.cells)
        );
    }
    t.advance(b"\x1bc");
    assert!(t.operation_metrics().is_some_and(|m| m[9] > 0));
    let counts = allocation_counter::measure(|| {
        t.start_operation_metrics();
        assert_eq!(t.operation_metrics(), Some([0; 10]));
    });
    assert_eq!(counts.count_total, 0);
}
