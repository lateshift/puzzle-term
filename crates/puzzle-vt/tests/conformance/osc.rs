//! OSC cases from https://invisible-island.net/xterm/ctlseqs/ctlseqs.html.

use puzzle_vt::{Terminal, terminal::TerminalEvent};

/// OSC 0 sets both labels; CSI 22/23 t restore a bounded prior title.
#[test]
fn title_stack_restores_previous_text() {
    let mut term = Terminal::new(8, 2, 0, 0);
    term.advance(b"\x1b]0;first\x07\x1b[22t\x1b]2;second\x1b\\\x1b[23t");
    assert_eq!(term.title, "first");
    assert_eq!(term.icon_name, "first");
}

/// OSC color queries retain BEL or ST and use four hex digits per channel.
#[test]
fn color_queries_preserve_request_terminator() {
    let mut term = Terminal::new(8, 2, 0, 0);
    term.advance(b"\x1b]4;1;#112233\x07\x1b]4;1;?\x07\x1b]10;rgb:1/2/3\x1b\\\x1b]10;?\x1b\\");
    assert_eq!(
        term.replies,
        b"\x1b]4;1;rgb:1111/2222/3333\x07\x1b]10;rgb:1111/2222/3333\x1b\\"
    );
}

/// OSC 8 link handles live on cells and close without rewriting old text.
#[test]
fn hyperlink_open_close_keeps_existing_cell() {
    let mut term = Terminal::new(8, 2, 0, 0);
    term.advance(b"\x1b]8;id=one;https://example.org\x1b\\A\x1b]8;;\x1b\\B");
    let row = term.screen().row(0).expect("row");
    let id = row.extras[0].link_id.expect("link handle");
    assert_eq!(
        term.links.resolve(puzzle_vt::links::LinkId(id)),
        Some("https://example.org")
    );
    assert!(row.extras.iter().all(|extra| extra.column != 1));
}

/// OSC 52 reads send no reply; writes remain policy events only.
#[test]
fn clipboard_read_denied_write_is_event() {
    let mut term = Terminal::new(8, 2, 0, 0);
    term.advance(b"\x1b]52;c;?\x07\x1b]52;c;aGVsbG8=\x07");
    assert!(term.replies.is_empty());
    assert!(
        term.events
            .contains(&TerminalEvent::ClipboardWrite("hello".into()))
    );
}

/// OSC 133 and VS Code's 633 alias create equivalent shell marks.
#[test]
fn shell_mark_aliases_and_progress() {
    let mut term = Terminal::new(8, 2, 0, 0);
    term.advance(b"\x1b]133;A\x07\x1b]633;C\x07\x1b]133;D;1\x07\x1b]9;4;2;75\x07");
    assert_eq!(term.zones.iter().count(), 3);
    assert!(
        term.events
            .contains(&TerminalEvent::CommandFinished(Some(1)))
    );
    assert!(term.events.contains(&TerminalEvent::Progress {
        state: 2,
        percent: 75
    }));
}
