//! Mandatory scalar/version work and optional absence are separate guarantees.
#![allow(
    clippy::expect_used,
    reason = "Missing fixture rows or evidence files must fail these regression assertions."
)]
use crate::{Terminal, osc::OscRequest, print_work, scroll_operations};

/// Frozen pre-clock store/rollover counts at both product review geometries.
#[test]
fn ordinary_print_retains_pre_clock_cell_and_version_operation_counts() {
    for (columns, rows) in [(120, 40), (282, 63)] {
        for (scalar, width) in [("A", 1), ("é", 1), ("漢", 2)] {
            let mut terminal = Terminal::new(columns, rows, 0, 0);
            terminal.advance(b"\x1b[?1049h");
            let scalars = columns * rows;
            let bytes = scalar.repeat(scalars);
            // Warm complete rows; the measured first scalar consumes pending wrap.
            terminal.advance(bytes.as_bytes());
            let mut work = [0; 7];
            let cells = scroll_operations::measure(|| {
                work = print_work::measure(|| terminal.advance(bytes.as_bytes()));
            });
            let stores = scalars * width;
            // Baseline versions include one wrap flag and one recycle per full row.
            assert_eq!(
                work,
                [
                    scalars as u64,
                    (stores + 2 * stores / columns) as u64,
                    1,
                    1,
                    0,
                    0,
                    0
                ]
            );
            assert_eq!(cells.written, stores);
            assert_eq!(cells.cleared, 0);
            assert_eq!(cells.block_lookups, 0);
            assert_eq!(terminal.source_watch_counts(), Default::default());
        }
    }
}

/// Reply admission examines callbacks, never each scalar in a plain output span.
#[test]
fn reply_fence_does_not_scan_queues_per_scalar() {
    for length in [1, 4096] {
        let mut terminal = Terminal::new(120, 40, 0, 0);
        let bytes = "漢".repeat(length);
        let mut request = OscRequest::default();
        let work = print_work::measure(|| {
            assert_eq!(
                terminal
                    .advance_to_reply_boundary(bytes.as_bytes(), &[], &mut request, None, false)
                    .0,
                bytes.len()
            );
        });
        assert_eq!(work[0], length as u64);
        // One entry and two parser-span fences, independent of scalar count.
        // vte scans a complete ground-state string between those fences.
        assert_eq!(work[3], 3);
    }
}

/// The complete approved corpus retains scalar/row work but stores no clock word.
#[test]
fn unregistered_unicode_corpus_has_zero_clock_stores() {
    let bytes = std::env::var_os("PUZZLE_UNICODE_CORPUS").map_or_else(
        || include_bytes!("../../../../../assets/fixtures/corpus/unicode-mix.vt").to_vec(),
        |path| std::fs::read(path).expect("required regression fixture"),
    );
    let mut receipt = String::from(
        "geometry,bytes,scalars,versions,watch_choices,reply_checks,edges,retries,clock_stores,cells,cleared,unicode_classifications,block_lookups\n",
    );
    for (columns, rows) in [(120, 40), (282, 63)] {
        let mut term = Terminal::new(columns, rows, 10_000, 32 * 1024 * 1024);
        term.advance(b"\x1b[?1049h");
        for slice in bytes.chunks(16 * 1024) {
            term.advance(slice);
        }
        let mut work = [0; 7];
        let cells = scroll_operations::measure(|| {
            work = print_work::measure(|| {
                for slice in bytes.chunks(16 * 1024) {
                    term.advance(slice);
                }
            });
        });
        assert_eq!(work[6], 0);
        assert_eq!(term.write_clock(), 0);
        receipt.push_str(&format!(
            "{columns}x{rows},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            bytes.len(),
            work[0],
            work[1],
            work[2],
            work[3],
            work[4],
            work[5],
            work[6],
            cells.written,
            cells.cleared,
            cells.unicode_classifications,
            cells.block_lookups
        ));
    }
    // Optional evidence serialization happens after counting; CI needs no artifact.
    if let Some(path) = std::env::var_os("PUZZLE_UNUSED_REPORT") {
        std::fs::write(path, receipt).expect("required regression fixture");
    }
}
