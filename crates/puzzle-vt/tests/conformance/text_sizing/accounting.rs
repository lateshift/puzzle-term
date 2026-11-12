//! Reviewer-supplied operations reproduce storage and fragment-consistency findings.
//! These are ordinary VT operations, independent of a fuzzer or opaque input.
use super::*;

#[test]
fn joined_block_charge_survives_edit_insert_line_and_reflow() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    for (step, bytes) in [
        (
            "1: OSC 66 placement",
            "\x1b]66;s=1:w=0:n=0:d=3:v=1:h=0;A一é\x1b\\",
        ),
        ("2: variation selector 19 joins é", "\u{e0102}"),
        ("3: delete character", "\x1b[P"),
        ("4: insert line", "\x1b[L"),
    ] {
        term.advance(bytes.as_bytes());
        // Separate boundaries identify the mutation responsible for divergence.
        assert_consistent(&term, step);
    }
    // IL moved the intact joined block to row 2. Reflow must retain its real
    // allocation, rather than changing the allocation to match a wrong bill.
    let joined = block(&term, 1, 3);
    let capacity = joined.text.capacity();
    let allocation = joined.text.as_ptr();
    assert_eq!(joined.text, "é\u{e0102}");
    term.resize(5, 3);
    assert_consistent(&term, "5: resize/reflow");
    let moved = block(&term, 1, 3);
    assert_eq!(moved.text, "é\u{e0102}");
    assert_eq!(moved.text.capacity(), capacity);
    assert_eq!(moved.text.as_ptr(), allocation);
}

#[test]
fn narrow_screen_blocks_survive_grow_and_insert_character() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    term.resize(2, 1);
    assert_consistent(&term, "1: shrink to two columns and one row");
    for (step, bytes) in [
        ("2a: carriage return and line feed", "\r\n"),
        ("2b: scroll up", "\x1b[S"),
        ("2c: scroll down", "\x1b[T"),
        ("3: wide ideograph fills the row", "\u{20f82}"),
        ("4: delete character", "\x1b[P"),
        (
            "5: automatic-width aligned blocks",
            "\x1b]66;s=1:w=0:n=0:d=0:v=0:h=2;A一é\x1b\\",
        ),
    ] {
        term.advance(bytes.as_bytes());
        // Check each constituent scroll separately as well as every user step.
        assert_consistent(&term, step);
    }
    term.resize(17, 2);
    assert_consistent(&term, "6: grow to seventeen columns and two rows");
    // Each payload must enter reflow's block seam, even beside the old wide
    // splice boundary. No copied slot key can stand in for a relocated record.
    for (column, text) in [(1, "A"), (2, "一"), (4, "é")] {
        let record = block(&term, 1, column);
        assert_eq!(record.text, text);
        assert_eq!(record.column, column);
        assert_eq!(
            record.fragments[0].line,
            term.screen().row(1).expect("row").id
        );
    }
    term.advance(b"\x1b[@");
    assert_consistent(&term, "7: insert character");
    let line = term.screen().row(1).expect("grown row").id;
    assert_eq!(term.buffer().text(line..=line, Default::default()), " A一é");
}

/// Validate both screen stores immediately after the named operation.
fn assert_consistent(term: &Terminal, step: &str) {
    super::super::check_grid(term);
    let (columns, rows) = term.size();
    assert!(
        term.cursor.column < columns && term.cursor.row < rows,
        "cursor outside screen after {step}"
    );
    #[cfg(debug_assertions)]
    for screen in [&term.primary, &term.alternate] {
        assert_eq!(screen.ring.check_multicells(), Ok(()), "after {step}");
    }
    // Release still exercises the public operation sequence and cell ABI.
    super::check(term);
}
