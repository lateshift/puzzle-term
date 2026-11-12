//! M18.7 ordinary printing never resolves a sparse block or allocates (P1).
#![allow(
    clippy::expect_used,
    reason = "fixtures and optional count receipts must report setup failures"
)]
use super::*;
/// Existing scalar fast paths retain their exact cell work in either width mode.
#[test]
fn text_sizing_ordinary_print_counts_and_allocations() {
    let mut csv = String::from(
        "columns,rows,clustering,text,written,cleared,block_lookups,anchors_examined\n",
    );
    for (columns, rows) in [(120, 40), (282, 63)] {
        for unicode in [false, true] {
            for text in ["A", "é", "漢"] {
                let mut term = Terminal::new(columns, rows, 0, 0);
                term.advance(if unicode {
                    b"\x1b[?2027h"
                } else {
                    b"\x1b[?2027l"
                });
                let bytes = text.repeat(columns * rows);
                // Warm recycled row backing before counting actual stores/clears.
                term.advance(bytes.as_bytes());
                term.advance(bytes.as_bytes());
                let counts = crate::scroll_operations::measure(|| term.advance(bytes.as_bytes()));
                assert_eq!(counts.block_lookups, 0, "{counts:?}");
                assert_eq!(counts.ordinary_block_probes, 0, "{counts:?}");
                assert_eq!(
                    counts.unicode_classifications,
                    if unicode && text != "A" {
                        columns * rows
                    } else {
                        0
                    },
                    "one existing predecessor classification per Unicode scalar: {counts:?}"
                );
                // Complete replacements initialize no redundant BCE slot;
                // both scalar and wide stores materialize only genuine gaps.
                assert_eq!(
                    counts.cleared, 0,
                    "{columns}/{rows}/{unicode}/{text}: {counts:?}"
                );
                assert_eq!(
                    counts.written,
                    columns * rows * if text == "漢" { 2 } else { 1 },
                    "{counts:?}"
                );
                csv.push_str(&format!(
                    "{columns},{rows},{unicode},{text},{},{},{},{}\n",
                    counts.written, counts.cleared, counts.block_lookups, counts.anchors_examined
                ));
                let allocated = allocation_counter::measure(|| term.advance(bytes.as_bytes()));
                assert_eq!(
                    allocated.count_total, 0,
                    "{columns}x{rows}/{unicode}/{text}: {allocated:?}"
                );
                assert!(term.screen().ring.multicells.is_none());
            }
        }
    }
    if let Some(path) = std::env::var_os("PUZZLE_VT_TEXT_COUNT_REPORT") {
        std::fs::write(path, csv).expect("count evidence");
    }
}
/// A live OSC 66 resolution reaches the counter, so absent-feature zeros are real.
#[test]
fn text_sizing_lookup_observer_detects_live_resolution() {
    let mut term = Terminal::new(12, 5, 0, 0);
    term.advance(b"\x1b]66;s=2;A\x07");
    let key = term.screen().row(0).expect("owner").cells[0].ch;
    let counts = crate::scroll_operations::measure(|| {
        assert_eq!(term.screen().ring.multicell(key).expect("block").text, "A");
    });
    assert_eq!(counts.block_lookups, 1);
}

/// The real Unicode stream observes predecessor dispatch as well as cell stores.
#[test]
fn text_sizing_unicode_corpus_counts() {
    // Every normal check exercises mixed scalars and joins; evidence runs may
    // substitute the immutable captured corpus without changing the observer.
    let bytes = std::env::var_os("PUZZLE_UNICODE_COUNT_CORPUS").map_or_else(
        || "é漢a\u{301}🧑\u{200d}💻\r\n".repeat(1024).into_bytes(),
        |path| std::fs::read(path).expect("host-supplied corpus"),
    );
    let mut csv = String::from(
        "columns,rows,bytes,written,cleared,handles,exchanges,anchors,unicode_classifications,ordinary_block_probes,block_lookups\n",
    );
    for (columns, rows) in [(120, 40), (282, 63)] {
        let mut terminal = Terminal::new(columns, rows, 0, 0);
        terminal.advance(b"\x1b[?1049h");
        terminal.advance(&bytes);
        let counts = crate::scroll_operations::measure(|| terminal.advance(&bytes));
        assert_eq!(counts.ordinary_block_probes, 0, "{counts:?}");
        csv.push_str(&format!(
            "{columns},{rows},{},{},{},{},{},{},{},{},{}\n",
            bytes.len(),
            counts.written,
            counts.cleared,
            counts.handles.len(),
            counts.exchanges,
            counts.anchors_examined,
            counts.unicode_classifications,
            counts.ordinary_block_probes,
            counts.block_lookups
        ));
    }
    if let Some(path) = std::env::var_os("PUZZLE_UNICODE_COUNT_REPORT") {
        std::fs::write(path, csv).expect("Unicode count receipt");
    }
}
