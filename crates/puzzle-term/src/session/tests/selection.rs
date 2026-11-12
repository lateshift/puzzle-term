//! Public selection commands guard stability, extraction and assembly (§6.9).
//!
//! Tests feed parser output and read Session APIs, rather than manipulating the
//! owned selection. Reflow cases use a real text context so the desktop resize
//! seam is exercised. A range must never silently copy newly rewritten content.
use super::*;
use crate::{
    frame_builder::context::TextContext,
    selection::SelectionCommand,
    text::{FontData, font_set::FontSet},
    vt::{
        selection::{SelectPoint, SelectionKind},
        tracked::LineId,
    },
};
use proptest::prelude::*;
/// Bundled outlines provide the same resize and row-cache behavior as the window.
pub(super) fn context() -> TextContext {
    let font = FontData::new(
        include_bytes!("../../../../../assets/fonts/JetBrainsMono-Variable.ttf").to_vec(),
        0,
    );
    TextContext::new(FontSet::new(font, None, None).expect("font"), 14.0, 1.0).expect("context")
}
/// Dispatches inclusive endpoints through the public bus lane and pumps now.
fn select(session: &mut TestHost, kind: SelectionKind, a: (LineId, usize), b: (LineId, usize)) {
    session.submit(
        Command::Select(SelectionCommand::Range {
            kind,
            start: SelectPoint {
                line: a.0,
                column: a.1,
            },
            end: SelectPoint {
                line: b.0,
                column: b.1,
            },
        }),
        Origin::Api,
    );
    session.pump(Instant::now());
}
/// Anchored history survives output, local scrolling and a primary reflow.
#[test]
fn selection_stability() {
    let (mut session, _, _) = fixture();
    let mut context = context();
    session.set_text_context(&context);
    let now = Instant::now();
    session.feed(b"abcdefghij\r\n", now);
    let id = *session.buffer().line_ids().expect("lines").start();
    select(&mut session, SelectionKind::Simple, (id, 2), (id, 7));
    session.submit(Command::Scroll(-2.0), Origin::User);
    session.pump(now);
    for _ in 0..40 {
        session.feed(b"unrelated\r\n", now);
    }
    assert_eq!(session.selection_text(), "cdefgh");
    session.resize_grid(4, 24, [100, 450]).expect("resize");
    assert_eq!(session.selection_text(), "cdefgh");
    // A selection-only change leaves prepared row geometry untouched.
    session.draw(now, &mut context).expect("first frame");
    session.submit(Command::Select(SelectionCommand::Clear), Origin::Api);
    session.pump(now);
    session.draw(now, &mut context).expect("clear frame");
    assert_eq!(session.builder.as_ref().expect("builder").rebuilt_rows(), 0);
}
/// Every in-place selected row rewrite, screen incarnation or RIS clears it.
#[test]
fn selection_clearing_rules() {
    for bytes in [
        b"\rY".as_slice(),
        b"\x1b[?1049h",
        b"\x1b[?1049h\x1b[?1049l",
        b"\x1bc",
    ] {
        let (mut session, _, _) = fixture();
        session.feed(b"hello", Instant::now());
        let id = session.buffer().cursor().expect("cursor").line;
        select(&mut session, SelectionKind::Simple, (id, 0), (id, 4));
        session.feed(bytes, Instant::now());
        assert!(session.selection_range().is_none(), "{bytes:?}");
    }
    // Triple click covers every wrapped row, so rewriting the unclicked part clears it.
    let (mut session, _, _) = fixture();
    session.terminal.resize(4, 3);
    session.feed(b"abcdefgh", Instant::now());
    let id = session.buffer().viewport().expect("viewport").first;
    select(&mut session, SelectionKind::Line, (id, 1), (id, 1));
    assert_eq!(session.selection_text(), "abcdefgh");
    session.feed(b"\x1b[2;1HZ", Instant::now());
    assert!(session.selection_range().is_none());
}
/// Copy uses the same expanded cells as highlighting, including trim-off padding.
#[test]
fn selection_extraction_rules() {
    let (mut session, _, _) = fixture();
    session.terminal.resize(6, 4);
    session.feed("ab界e\u{301}fg\r\nhi  ".as_bytes(), Instant::now());
    let ids: Vec<_> = session
        .buffer()
        .lines({
            let v = session.buffer().viewport().expect("viewport");
            v.first..=v.last
        })
        .map(|r| r.id())
        .collect();
    select(
        &mut session,
        SelectionKind::Simple,
        (ids[0], 0),
        (ids[2], 5),
    );
    assert_eq!(session.selection_text(), "ab界e\u{301}fg\nhi");
    session.apply_settings(
        &crate::settings::Settings {
            selection_trim_trailing: false,
            ..Default::default()
        },
        Instant::now(),
    );
    assert_eq!(session.selection_text(), "ab界e\u{301}fg     \nhi    ");
    select(&mut session, SelectionKind::Block, (ids[0], 0), (ids[2], 1));
    assert_eq!(session.selection_text(), "ab\ng \nhi\n");
    session.feed(b"\x1bcfoo/bar!", Instant::now());
    let id = session.buffer().viewport().expect("viewport").first;
    session.apply_settings(
        &crate::settings::Settings {
            selection_word_separators: "/!".into(),
            ..Default::default()
        },
        Instant::now(),
    );
    select(&mut session, SelectionKind::Word, (id, 1), (id, 1));
    assert_eq!(session.selection_text(), "foo");
}
/// Semantic output and select-all actions run without writing application input.
#[test]
fn selection_actions_and_markers() {
    let (mut session, writes, _) = fixture();
    session.feed(
        b"prompt \x1b]133;C\x07output\x1b]133;D;0\x07 next",
        Instant::now(),
    );
    session.submit(
        Command::BindingAction {
            action: crate::keybind::Action::SelectCommandOutput,
            all: false,
        },
        Origin::User,
    );
    session.pump(Instant::now());
    assert_eq!(session.selection_text(), "output");
    assert!(
        session
            .viewport()
            .scrollbar
            .markers()
            .iter()
            .any(|m| m.kind == crate::viewport::scrollbar::MarkerKind::Selection)
    );
    session.submit(
        Command::BindingAction {
            action: crate::keybind::Action::SelectAll,
            all: false,
        },
        Origin::User,
    );
    session.pump(Instant::now());
    assert!(session.selection_text().starts_with("prompt output next"));
    assert!(writes.lock().expect("writes").is_empty());
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    /// Reflow remaps a historical selection across random widths/output without changing its text.
    #[test]
    fn selection_random_output_and_resizes(widths in prop::collection::vec(3usize..35,1..8),
        output in "[a-z]{1,20}") {
        let (mut session,_,_) = fixture(); let context = context(); session.set_text_context(&context);
        let now = Instant::now(); session.feed(b"abcdefghijklmnopqrst\r\n",now);
        let id = *session.buffer().line_ids().expect("lines").start();
        select(&mut session,SelectionKind::Simple,(id,2),(id,15));
        for _ in 0..32 { session.feed(format!("{output}\r\n").as_bytes(),now); }
        for width in widths {
            session.resize_grid(width,24,[400,450]).expect("resize");
            prop_assert!(session.selection_range().is_some());
            prop_assert_eq!(session.selection_text(),"cdefghijklmnop");
        }
    }
}

/// D at column zero copies the output newline without selecting the next prompt.
#[test]
fn semantic_output_final_newline_and_same_row_boundaries() {
    let (mut session, _, _) = fixture();
    let now = Instant::now();
    session.feed(
        b"\x1b]133;C\x07output\r\n\x1b]133;D;0\x07\x1b]133;A\x07prompt",
        now,
    );
    session.submit(
        Command::Select(SelectionCommand::CommandOutput(None)),
        Origin::Api,
    );
    session.pump(now);
    assert_eq!(session.selection_text(), "output\n");
    let range = session.selection_range().expect("output");
    assert_ne!(
        range.end.line,
        session.buffer().cursor().expect("prompt").line
    );
    // Editing the unselected next prompt must not invalidate the finished output.
    session.feed(b"\rPROMPT", now);
    assert_eq!(session.selection_text(), "output\n");
    // A semantic point before C on the same physical row cannot select future output.
    session.feed(b"\x1bcabc\x1b]133;C\x07result\x1b]133;D;0\x07", now);
    let id = session.buffer().cursor().expect("cursor").line;
    select(&mut session, SelectionKind::Semantic, (id, 1), (id, 1));
    assert!(session.selection_range().is_none());
}

/// Double click traverses a soft wrap and a spacer click copies the full wide cluster.
#[test]
fn word_soft_wrap_and_wide_spacer() {
    let (mut session, _, _) = fixture();
    session.terminal.resize(6, 4);
    session.feed(" abcdefgh 界".as_bytes(), Instant::now());
    let ids: Vec<_> = session
        .buffer()
        .lines({
            let v = session.buffer().viewport().expect("viewport");
            v.first..=v.last
        })
        .map(|row| row.id())
        .collect();
    select(&mut session, SelectionKind::Word, (ids[1], 1), (ids[1], 1));
    assert_eq!(session.selection_text(), "abcdefgh");
    select(
        &mut session,
        SelectionKind::Simple,
        (ids[1], 5),
        (ids[1], 5),
    );
    assert_eq!(session.selection_text(), "界");
    let range = session.selection_range().expect("wide range");
    assert_eq!((range.start.column, range.end.column), (4, 5));
}
