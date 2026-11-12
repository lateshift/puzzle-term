//! OSC 99 fixtures exercise the actual VT parser, including split UTF-8/base64.
use super::*;
/// Inspect only completed notification requests, never intermediate chunks.
fn requests(term: &Terminal) -> Vec<&Request> {
    term.events
        .iter()
        .filter_map(|e| match e {
            TerminalEvent::DesktopNotification(n) => Some(n),
            _ => None,
        })
        .collect()
}
#[test]
fn chunks_interleave_and_preserve_metadata() {
    let mut t = Terminal::new(80, 24, 10, 100_000);
    t.advance(b"\x1b]99;i=a:d=0:u=2:a=report:c=1;Title\x1b\\hello\x1b]99;i=b;Other\x1b\\\x1b]99;i=a:p=body;Body;tail\x1b\\");
    let r = requests(&t);
    assert_eq!(r.len(), 2);
    let Request::Post(n) = r[1] else {
        panic!("post")
    };
    assert_eq!(
        (&n.title, &n.body),
        (&"Title".to_owned(), &"Body;tail".to_owned())
    );
    assert_eq!(n.urgency, 2);
    assert!(n.focus && n.report && n.close_report);
}
#[test]
fn encoded_stream_can_split_at_any_symbol_or_use_independent_padding() {
    for packets in [
        "\x1b]99;i=x:e=1:d=0;4p\x1b\\\x1b]99;i=x:e=1;yT\x1b\\",
        "\x1b]99;i=x:e=1:d=0;YQ==\x1b\\\x1b]99;i=x:e=1;Yg==\x1b\\",
    ] {
        let mut t = Terminal::new(80, 24, 0, 0);
        for b in packets.as_bytes() {
            t.advance(&[*b]);
        }
        let r = requests(&t);
        let Request::Post(n) = r[0] else {
            panic!("post")
        };
        assert!(n.title == "✓" || n.title == "ab");
    }
}
#[test]
fn limits_reset_close_and_query_do_not_swallow_output() {
    let mut t = Terminal::new(80, 24, 0, 0);
    t.advance(format!("\x1b]99;i=x;{}\x1b\\", "a".repeat(2049)).as_bytes());
    assert!(requests(&t).is_empty());
    t.advance(b"\x1b]99;i=a:d=0;lost\x1b\\\x1bc\x1b]99;i=a;new\x1b\\\x1b]99;i=a:p=close;\x1b\\\x1b]99;i=q:p=?;\x1b\\\x1b[c");
    assert_eq!(requests(&t).len(), 2);
    assert!(t.replies.starts_with(b"\x1b]99;i=q:p=?;a=focus,report"));
    assert!(t.replies.ends_with(b"\x1b[?62;22c"));
    assert!(!String::from_utf8_lossy(&t.replies).contains("icon"));
}
#[test]
fn raw_semicolons_and_controls_are_handled_without_parser_truncation() {
    let mut t = Terminal::new(80, 24, 0, 0);
    let title = "word;".repeat(30);
    t.advance(format!("\x1b]99;;{title}\x1b\\").as_bytes());
    let r = requests(&t);
    let Request::Post(n) = r[0] else {
        panic!("post")
    };
    assert_eq!(n.title, title);
    t.events.clear();
    // vte strips this control; strict base64 must still reject the original input.
    t.advance(b"\x1b]99;e=1;Y\x01Q==\x1b\\");
    assert!(requests(&t).is_empty());
}

#[test]
fn invalid_encoding_and_hostile_metadata_cannot_create_requests() {
    for packet in [
        "e=1;YQ==Yg==",
        "e=1;YR==",
        "e=1;A",
        "i=bad/id;title",
        "u=9;title",
    ] {
        let mut t = Terminal::new(80, 24, 0, 0);
        t.advance(format!("\x1b]99;{packet}\x1b\\OK").as_bytes());
        assert!(requests(&t).is_empty(), "{packet}");
        assert_eq!(t.cursor.column, 2);
    }
}

#[test]
fn unfinished_table_and_decoded_fields_stay_bounded() {
    let mut t = Terminal::new(80, 24, 0, 0);
    // A sender cannot retain arbitrary abandoned identifiers by never completing them.
    for i in 0..80 {
        t.advance(format!("\x1b]99;i=n{i}:d=0;part\x1b\\").as_bytes());
    }
    assert_eq!(
        t.host_protocols
            .notifications
            .as_ref()
            .map(|state| state.entries.len()),
        Some(MAX_NOTIFICATIONS)
    );
    assert!(requests(&t).is_empty());
    t.advance(b"\x1bc");
    assert!(t.host_protocols.notifications.is_none());
    // Each legal packet is below the packet limit, but their assembled sum is not.
    for _ in 0..3 {
        t.advance(format!("\x1b]99;i=large:d=0;{}\x1b\\", "a".repeat(2048)).as_bytes());
    }
    assert!(
        t.host_protocols
            .notifications
            .as_ref()
            .is_some_and(|state| state.entries.is_empty())
    );
    assert!(requests(&t).is_empty());
    // The public scalar cap applies even when a byte-limited packet is otherwise valid.
    t.advance(format!("\x1b]99;;{}\x1b\\", "a".repeat(1500)).as_bytes());
    let Request::Post(n) = requests(&t)[0] else {
        panic!("post")
    };
    assert_eq!(n.title.chars().count(), MAX_TEXT);
}
