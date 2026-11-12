//! HTML copy regression cases from the reviewer’s M8.5 notes (§14.9).
#![allow(
    clippy::expect_used,
    reason = "test assertions identify required retained cells"
)]
use super::*;

/// Both public exports must resolve decorations, inverse/faint and merged runs.
#[test]
fn html_exports_share_resolved_runs() {
    let mut term = Terminal::new(12, 1, 100, 1_000_000);
    // Equivalent palette/RGB colors must merge after resolution, not by raw tag.
    term.advance(b"\x1b]4;1;#c86432\x07\x1b[31;48;2;20;40;60mAB\x1b[38;2;200;100;50mC\x1b[4;9mD\x1b[24;29;7mE\x1b[2mF\x1b[0m<&");
    let id = *term.buffer().line_ids().expect("retained row").start();
    let range = CellRange {
        start: (id, 0),
        end: (id, 8),
        block: false,
        trim_trailing: true,
        final_newline: false,
    };
    for html in [
        term.buffer().serialize(id..=id, Format::Html),
        term.buffer().serialize_cells(range, Format::Html),
    ] {
        assert!(
            html.contains("text-decoration:underline line-through"),
            "{html}"
        );
        assert!(
            html.contains("color:#14283c;background-color:#c86432\">E"),
            "{html}"
        );
        assert!(
            html.contains("color:#6e4637;background-color:#c86432\">F"),
            "{html}"
        );
        assert!(html.contains(">ABC</span>"), "{html}");
        assert_eq!(html.matches("<span ").count(), 5, "{html}");
        assert!(html.contains("&lt;&amp;"), "{html}");
    }
}

/// A physical soft wrap must not split an otherwise identical resolved HTML run.
#[test]
fn html_runs_join_soft_wrap_and_preserve_hidden() {
    let mut term = Terminal::new(4, 3, 0, 0);
    term.advance(b"abcdef\r\n\x1b[8mxy");
    let ids = term.buffer().line_ids().expect("rows");
    for html in [
        term.buffer().serialize(ids.clone(), Format::Html),
        term.buffer().serialize_cells(
            CellRange {
                start: (*ids.start(), 0),
                end: (*ids.end(), 2),
                block: false,
                trim_trailing: true,
                final_newline: false,
            },
            Format::Html,
        ),
    ] {
        assert!(html.contains(">abcdef"));
        assert!(html.contains("visibility:hidden"));
        assert!(html.contains("</span>\n<span"));
    }
}
