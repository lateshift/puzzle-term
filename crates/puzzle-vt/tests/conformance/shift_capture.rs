//! Xterm XTSHIFTESCAPE; common-base CSI ownership negotiation (M18.2).
//! Reference: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html
use puzzle_vt::Terminal;

/// Omitted/zero release, one captures; invalid forms cannot change prior state.
#[test]
fn shift_capture_grammar_and_reset() {
    let mut term = Terminal::new(20, 4, 0, 0);
    assert_eq!(term.mouse_shift_capture, None);
    term.advance(b"\x1b[>1s");
    assert_eq!(term.mouse_shift_capture, Some(true));
    for invalid in [
        b"\x1b[>2s".as_slice(),
        b"\x1b[>0;1s",
        b"\x1b[>0:1s",
        b"\x1b[?0s",
    ] {
        term.advance(invalid);
        assert_eq!(term.mouse_shift_capture, Some(true));
    }
    // DECSTR resets tracking but preserves the independent program preference,
    // as a soft reset does.
    term.advance(b"\x1b[!p");
    assert_eq!(term.mouse_shift_capture, Some(true));
    term.advance(b"\x1b[>s");
    assert_eq!(term.mouse_shift_capture, Some(false));
    term.advance(b"\x1b[>1s\x1b[>0s");
    assert_eq!(term.mouse_shift_capture, Some(false));
    term.advance(b"\x1b[>1s\x1bcZ");
    assert_eq!(term.mouse_shift_capture, None);
    assert_eq!(super::row(&term, 0).chars().next(), Some('Z'));
}

/// Negotiating an unused feature has no allocation and never emits reply bytes.
#[test]
fn shift_capture_warm_negotiation_allocates_nothing() {
    let mut term = Terminal::new(20, 4, 0, 0);
    term.advance(b"\x1b[>1s\x1b[>0s");
    let measured = allocation_counter::measure(|| {
        for _ in 0..1000 {
            term.advance(b"\x1b[>1s\x1b[>0s");
        }
    });
    assert_eq!(measured.count_total, 0);
    assert!(term.replies.is_empty());
}
