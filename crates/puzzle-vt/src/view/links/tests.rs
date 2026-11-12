//! On-demand URL detection cases from design §6.11.
//!
//! A URL broken by autowrap remains one target. Terminal output cannot
//! activate it; the host applies opening policy after lookup.
use super::*;

/// A plain URL crosses a soft wrap and drops sentence punctuation.
#[test]
fn url_detection_crosses_wrap_and_trims_punctuation() {
    let mut term = Terminal::new(12, 3, 0, 0);
    term.advance(b"See https://example.org/path. now");
    let Some(first) = term.screen().row(0).map(|row| row.id) else {
        panic!("row")
    };
    let view = term.buffer();
    assert_eq!(
        view.link_at(first, 7),
        Some("https://example.org/path".to_string())
    );
    let Some(ids) = view.line_ids() else {
        panic!("retained IDs")
    };
    assert!(
        view.links(ids)
            .any(|(_, _, uri)| uri == "https://example.org/path")
    );
}

/// An id joins disjoint explicit segments; wide URL characters include spacers.
#[test]
fn grouped_link_target_and_detected_wide_cells() {
    let mut term = Terminal::new(30, 4, 0, 0);
    term.advance(b"\x1b]8;id=x;https://a\x1b\\AB\x1b]8;;\x1b\\\r\n\x1b]8;id=x;https://b\x1b\\CD\x1b]8;;\x1b\\");
    let view = term.buffer();
    let Some(ids) = view.line_ids() else {
        panic!("rows")
    };
    let first = *ids.start();
    let Some(target) = view.link_target(first, 0, ids, true) else {
        panic!("group")
    };
    assert_eq!(target.uri, "https://a");
    assert_eq!(target.cells.len(), 4);
    term.advance("\r\nhttps://a/界.".as_bytes());
    let view = term.buffer();
    let Some(ids) = view.line_ids() else {
        panic!("rows")
    };
    let row = LineId(first.0 + 2);
    let Some(target) = view.link_target(row, 10, ids.clone(), true) else {
        panic!("wide URL")
    };
    assert_eq!(target.uri, "https://a/界");
    assert!(target.cells.contains(&(row, 11)));
    assert!(view.link_target(row, 1, ids, false).is_none());
}

/// Hover source changes reuse scratch and agree with the existing cold contents API.
#[test]
fn warm_hover_query_reuses_scratch_and_matches_contents() {
    let mut term = Terminal::new(20, 8, 0, 0);
    term.advance("See (https://example.org/界). mailto:a@b.test plain\r\n\x1b]8;id=x;https://a\x1b\\AB\x1b]8;;\x1b\\".as_bytes());
    let view = term.buffer();
    let Some(ids) = view.line_ids() else {
        panic!("rows")
    };
    let mut target = LinkTarget::default();
    // Sweep all source cells once to warm every logical-line and target capacity.
    for row in view.lines(ids.clone()) {
        for column in 0..20 {
            let found = view.link_target_into(row.id(), column, ids.clone(), true, &mut target);
            let lead = if row.cell(column).is_some_and(|cell| cell.width == 0) {
                column.saturating_sub(1)
            } else {
                column
            };
            assert_eq!(
                found.then(|| target.uri.clone()),
                view.link_at(row.id(), lead)
            );
        }
    }
    let measured = allocation_counter::measure(|| {
        // Unchanged and replaced sources share this path; the caller owns all scratch.
        for row in view.lines(ids.clone()) {
            for column in 0..20 {
                view.link_target_into(row.id(), column, ids.clone(), true, &mut target);
            }
        }
    });
    assert_eq!(measured.count_total, 0);
}
