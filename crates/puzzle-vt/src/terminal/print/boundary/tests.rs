//! Differential UAX #29 and terminal-width checks against the pre-M16.5 path.
//! Conformance strings come from the already-resolved approved dependency's
//! own Unicode 17 test source. No new downloaded or vendored fixture is needed.
#![allow(
    clippy::expect_used,
    reason = "test fixtures and reference assertions fail explicitly"
)]
use super::{Property, State};
use crate::Terminal;
use std::path::PathBuf;
use unicode_segmentation::UnicodeSegmentation;

/// Finds the exact approved dependency source Cargo unpacked for this build.
fn conformance_source() -> String {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").expect("home")).join(".cargo"));
    for registry in std::fs::read_dir(cargo_home.join("registry/src")).expect("Cargo sources") {
        let path = registry
            .expect("registry directory")
            .path()
            .join("unicode-segmentation-1.13.3/tests/testdata/mod.rs");
        if path.exists() {
            return std::fs::read_to_string(path).expect("approved conformance source");
        }
    }
    panic!("resolved unicode-segmentation 1.13.3 source is missing");
}

/// Decodes only the generated Rust Unicode literals in the conformance source.
fn conformance_cases() -> Vec<String> {
    let source = conformance_source();
    let graphemes = source
        .split("pub const TEST_WORD")
        .next()
        .expect("grapheme section");
    graphemes
        .split("(\"")
        .skip(1)
        .map(|entry| {
            let escaped = entry.split('"').next().expect("test string");
            escaped
                .split("\\u{")
                .skip(1)
                .map(|piece| {
                    let number = piece.split('}').next().expect("hex scalar");
                    char::from_u32(u32::from_str_radix(number, 16).expect("hex")).expect("scalar")
                })
                .collect()
        })
        .collect()
}

/// Collects streaming breaks without requiring string ownership in the machine.
fn incremental_boundaries(text: &str) -> Vec<usize> {
    let mut result = Vec::new();
    let mut state: Option<State> = None;
    for (offset, scalar) in text.char_indices() {
        let property = Property::of(scalar);
        if state.is_none_or(|state| !state.joins(property)) {
            result.push(offset);
            state = Some(State::start(property));
        } else {
            state.as_mut().expect("existing cluster").push(property);
        }
    }
    result
}

/// GB9c/11/12 context is preserved across every Unicode conformance boundary.
#[test]
fn unicode_17_conformance_matches_current_segmentation() {
    assert_eq!(unicode_segmentation::UNICODE_VERSION, (17, 0, 0));
    let cases = conformance_cases();
    assert!(
        cases.len() > 700,
        "the complete grapheme sections must be read"
    );
    for text in cases {
        let expected: Vec<_> = text
            .grapheme_indices(true)
            .map(|(offset, _)| offset)
            .collect();
        assert_eq!(incremental_boundaries(&text), expected, "{text:?}");
    }
}

/// Compares all grid cells, sparse clusters and cursor widths after every feed.
fn compare_terminal(bytes: &[u8], chunk: usize, width: usize) {
    let mut incremental = Terminal::new(width, 5, 0, 0);
    let mut reference = Terminal::new(width, 5, 0, 0);
    reference.grapheme.reference = true;
    for bytes in bytes.chunks(chunk) {
        incremental.advance(bytes);
        reference.advance(bytes);
        assert_eq!(
            incremental.cursor, reference.cursor,
            "cursor after {bytes:?}"
        );
        for row in 0..5 {
            let actual = incremental.screen().row(row).expect("row");
            let expected = reference.screen().row(row).expect("reference row");
            assert_eq!(actual.cells, expected.cells, "cells after {bytes:?}");
            assert_eq!(actual.extras, expected.extras, "clusters after {bytes:?}");
        }
    }
}

/// Original terminal behavior (including C0, ASCII gates and D-31) is unchanged.
#[test]
fn conformance_grid_and_widths_match_original_terminal() {
    for text in conformance_cases() {
        for width in [4, 32] {
            compare_terminal(text.as_bytes(), 1, width);
        }
    }
    // Cursor moves, legacy extras, reset, alternate screens and rejected
    // right-margin widening must all invalidate or recover the suffix witness.
    for text in [
        "👩‍💻\x1b[1G\x1b[2C️",
        "\x1b[?2027l👩‍💻\x1b[?2027h️",
        "abc✌︎\x1b[?7l️️X",
        "👩‍💻\x1bc👩‍💻",
        "👩‍💻\x1b[?1049h👩‍💻\x1b[?1049l️",
    ] {
        compare_terminal(text.as_bytes(), 1, 4);
    }
}

/// The local vtebench corpus is required when requested by the measurement gate.
#[test]
fn unicode_corpus_matches_original_terminal() {
    let bytes = std::env::var_os("PUZZLE_UNICODE_CORPUS")
        .map(|path| std::fs::read(path).expect("requested Unicode corpus"))
        .unwrap_or_else(|| {
            include_bytes!("../../../../../../assets/fixtures/corpus/unicode-mix.vt").to_vec()
        });
    // Splitting inside UTF-8 and at PTY/parse-slice boundaries protects the
    // state machine's independence from producer and transport chunk sizes.
    for chunk in [1, 1024, 16384] {
        compare_terminal(&bytes, chunk, 282);
    }
}

/// The actual repeatedly parsed Unicode workload obeys P1 after row warm-up.
#[test]
fn warm_unicode_corpus_allocates_nothing() {
    let bytes = std::env::var_os("PUZZLE_UNICODE_CORPUS")
        .map(|path| std::fs::read(path).expect("requested Unicode corpus"))
        .unwrap_or_else(|| {
            include_bytes!("../../../../../../assets/fixtures/corpus/unicode-mix.vt").to_vec()
        });
    let mut terminal = Terminal::new(282, 63, 0, 0);
    // Input may stop at any column, so repeated corpora distribute their rare
    // clusters differently over recycled rows. Reach one full ring of zero-
    // allocation passes, retaining a finite cursor-phase bound on warm-up.
    let mut warm = 0;
    for _ in 0..terminal.size().0 * terminal.size().1 {
        let sample = allocation_counter::measure(|| terminal.advance(&bytes));
        warm = if sample.count_total == 0 { warm + 1 } else { 0 };
        if warm == terminal.size().1 {
            break;
        }
    }
    assert_eq!(
        warm,
        terminal.size().1,
        "cluster storage must reach steady capacity"
    );
    let allocations = allocation_counter::measure(|| terminal.advance(&bytes));
    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_total, 0);
}
