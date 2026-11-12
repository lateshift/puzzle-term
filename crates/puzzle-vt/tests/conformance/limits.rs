//! §16 limits checked through the untrusted Terminal::advance byte surface.

use proptest::prelude::*;
use puzzle_vt::Terminal;

/// Title text and its stack remain bounded under repeated OSC writes.
#[test]
fn title_and_title_stack_caps() {
    let mut term = Terminal::new(8, 2, 0, 0);
    let payload = format!("\x1b]2;{}\x07", "X".repeat(70_000));
    term.advance(payload.as_bytes());
    assert!(term.title.chars().count() <= 1024);
    for _ in 0..100 {
        term.advance(b"\x1b[22t");
    }
    assert_eq!(term.title_stack.len(), 10);
}

/// A truncated OSC 8 or 52 cannot create a shortened link or clipboard event.
#[test]
fn truncated_link_and_clipboard_are_rejected() {
    let mut term = Terminal::new(8, 2, 0, 0);
    term.advance(format!("\x1b]8;;{}\x07", "a".repeat(70_000)).as_bytes());
    assert_eq!(term.current_link, None);
    term.advance(format!("\x1b]52;c;{}\x07", "A".repeat(70_000)).as_bytes());
    assert!(term.events.is_empty());
}

/// The link table and shell-zone list never grow past their §16 ceilings.
#[test]
fn link_and_zone_count_caps() {
    let mut term = Terminal::new(8, 2, 0, 0);
    for index in 0..4100 {
        term.links
            .insert(&format!("https://example.org/{index}"), None);
    }
    assert_eq!(term.links.len(), 4096);
    for _ in 0..10_010 {
        term.advance(b"\x1b]133;A\x07");
    }
    assert_eq!(term.zones.iter().count(), 10_000);
}

/// URI bytes beyond §16's 2 KiB ceiling never enter the link table.
#[test]
fn uri_and_dcs_caps() {
    let mut term = Terminal::new(8, 2, 0, 0);
    term.advance(format!("\x1b]8;;https://{}\x07", "x".repeat(2048)).as_bytes());
    assert!(term.links.is_empty());
    term.advance(format!("\x1bP$q{}\x1b\\", "m".repeat(4097)).as_bytes());
    assert!(term.replies.is_empty());
}

proptest! {
    /// Oversized malformed OSC/DCS input never panics or grows a link URI.
    #[test]
    fn oversized_control_strings_are_safe(bytes in proptest::collection::vec(any::<u8>(), 65_536..66_000)) {
        let mut term = Terminal::new(8, 2, 0, 0);
        term.advance(b"\x1b]8;;");
        term.advance(&bytes);
        term.advance(b"\x07");
        super::check_grid(&term);
        assert!(term.current_link.is_none_or(|id| term.links.resolve(puzzle_vt::links::LinkId(id)).is_some_and(|uri| uri.len() <= 2048)));
    }
}
