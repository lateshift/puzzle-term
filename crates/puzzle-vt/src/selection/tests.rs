//! Copy selection rules exercised on actual parser-produced rows (§6.9).
//!
//! Scrolling changes viewport rows but not selected line identities;
//! rewriting a covered row changes its version and invalidates the copy.
use super::*;

/// Block copy keeps physical line breaks while simple copy joins a wrap.
#[test]
fn block_copy_preserves_rows_and_simple_copy_joins_wrap() {
    let mut term = Terminal::new(4, 3, 0, 0);
    term.advance(b"abcdef");
    let Some(first) = term.screen().row(0).map(|row| row.id) else {
        panic!("first row")
    };
    let Some(second) = term.screen().row(1).map(|row| row.id) else {
        panic!("second row")
    };
    let a = SelectPoint {
        line: first,
        column: 2,
    };
    let b = SelectPoint {
        line: second,
        column: 1,
    };
    let Some(simple) = Selection::new(&mut term, SelectionKind::Simple, a, b) else {
        panic!("valid selection")
    };
    assert_eq!(simple.text(&term, ""), Some("cdef".to_string()));
    simple.clear(&mut term);
    let Some(block) = Selection::new(&mut term, SelectionKind::Block, a, b) else {
        panic!("valid block")
    };
    assert_eq!(block.text(&term, ""), Some("bc\nf\n".to_string()));
    block.clear(&mut term);
}

/// An in-place overwrite makes the old selection stale before copy.
#[test]
fn row_version_rewrite_invalidates_selection() {
    let mut term = Terminal::new(5, 2, 0, 0);
    term.advance(b"hello");
    let Some(line) = term.screen().row(0).map(|row| row.id) else {
        panic!("row")
    };
    let point = SelectPoint { line, column: 0 };
    let Some(selection) = Selection::new(&mut term, SelectionKind::Line, point, point) else {
        panic!("valid selection")
    };
    term.advance(b"\rY");
    assert!(selection.text(&term, "").is_none());
    selection.clear(&mut term);
}

/// Primary reflow remaps anchors but must leave the selected text intact.
#[test]
fn selection_survives_primary_reflow_without_rewrite() {
    let mut term = Terminal::new(8, 3, 3, 1024);
    term.advance(b"abcdef");
    let Some(line) = term.screen().row(0).map(|row| row.id) else {
        panic!("row")
    };
    let first = SelectPoint { line, column: 1 };
    let last = SelectPoint { line, column: 4 };
    let Some(selection) = Selection::new(&mut term, SelectionKind::Simple, first, last) else {
        panic!("valid selection")
    };
    assert_eq!(selection.text(&term, ""), Some("bcde".to_string()));
    term.resize(4, 3);
    assert_eq!(selection.text(&term, ""), Some("bcde".to_string()));
    selection.clear(&mut term);
}

/// A rewrite after reflow still invalidates the original highlight.
#[test]
fn selected_row_rewrite_after_reflow_is_rejected() {
    let mut term = Terminal::new(8, 3, 3, 1024);
    term.advance(b"abcdef");
    let Some(line) = term.screen().row(0).map(|row| row.id) else {
        panic!("row")
    };
    let first = SelectPoint { line, column: 1 };
    let last = SelectPoint { line, column: 4 };
    let Some(selection) = Selection::new(&mut term, SelectionKind::Simple, first, last) else {
        panic!("valid selection")
    };
    term.resize(4, 3);
    term.advance(b"\x1b[1;2HZ");
    assert!(selection.text(&term, "").is_none());
    selection.clear(&mut term);
}

/// Word separators and triple-click lines have distinct expansion rules.
#[test]
fn word_and_line_selection_expand_from_parser_rows() {
    let mut term = Terminal::new(8, 3, 0, 0);
    term.advance(b"foo/bar!");
    let Some(line) = term.screen().row(0).map(|row| row.id) else {
        panic!("row")
    };
    let point = SelectPoint { line, column: 1 };
    let Some(word) = Selection::new(&mut term, SelectionKind::Word, point, point) else {
        panic!("word")
    };
    assert_eq!(word.text(&term, "/!"), Some("foo".to_string()));
    word.clear(&mut term);
    let Some(whole) = Selection::new(&mut term, SelectionKind::Line, point, point) else {
        panic!("line")
    };
    assert_eq!(whole.text(&term, ""), Some("foo/bar!".to_string()));
    whole.clear(&mut term);
}

/// OSC 133 C/D boundaries extract only command output, even on one row.
#[test]
fn semantic_selection_uses_shell_boundary_columns() {
    let mut term = Terminal::new(20, 2, 0, 0);
    term.advance(b"prompt ");
    term.advance(b"\x1b]133;C\x07output\x1b]133;D;0\x07 next");
    let Some(line) = term.screen().row(0).map(|row| row.id) else {
        panic!("row")
    };
    let point = SelectPoint { line, column: 9 };
    let Some(selection) = Selection::new(&mut term, SelectionKind::Semantic, point, point) else {
        panic!("semantic")
    };
    assert_eq!(selection.text(&term, ""), Some("output".to_string()));
    let Some(mark) = term
        .buffer()
        .zones()
        .find(|mark| mark.kind == crate::zones::ZoneKind::Output)
    else {
        panic!("zone")
    };
    assert_eq!(
        term.buffer().command_output(mark),
        Some("output".to_string())
    );
    selection.clear(&mut term);
}
