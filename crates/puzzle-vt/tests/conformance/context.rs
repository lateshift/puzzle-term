//! UAPI.15 v1.0 active-context semantics through the public byte parser.
//! Reference: https://uapi-group.org/specifications/specs/osc_context/
//! Resets do not end SSH/privilege contexts; overflowing children cannot hide them.
use puzzle_vt::{
    Terminal,
    context::{FieldKey, MAX_DEPTH},
};

/// Feed one OSC using its real framing, preserving byte-split parser coverage.
fn osc(term: &mut Terminal, body: &str) {
    term.advance(format!("\x1b]3008;{body}\x1b\\").as_bytes());
}

#[test]
fn hierarchy_replacement_end_and_ris_preservation() {
    let mut term = Terminal::new(20, 4, 0, 0);
    osc(&mut term, "start=root;type=shell;user=test");
    osc(&mut term, "start=ssh;type=remote;targethost=remote.example");
    osc(&mut term, "start=priv;type=elevate;targetuser=root");
    assert_eq!(term.buffer().contexts().len(), 3);
    term.advance(b"\x1bc\x1b[!p\x1b[?1049h\x1b[?1049l");
    assert_eq!(term.contexts().len(), 3);
    // Returning to a parent clears all prior metadata and ends descendants.
    osc(&mut term, "start=ssh;type=remote");
    assert_eq!(term.contexts().len(), 2);
    assert_eq!(term.contexts()[1].get(FieldKey::TargetHost), None);
    osc(&mut term, "end=missing;exit=success");
    assert_eq!(term.contexts().len(), 2);
    osc(&mut term, "end=root;exit=failure;status=1;signal=SIGTERM");
    assert!(term.contexts().is_empty());
    assert!(term.replies.is_empty());
}

#[test]
fn all_fields_and_escaped_utf8_survive_byte_splits() {
    let mut term = Terminal::new(20, 4, 0, 0);
    let body = concat!(
        "\x1b]3008;start=a\\x3bb\\x5cc;type=command;user=test;hostname=host;",
        "machineid=3deb5353d3ba43d08201c136a47ead7b;bootid=d4a3d0fdf2e24fdea6d971ce73f4fbf2;",
        "pid=18446744073709551615;pidfdid=123;comm=zsh;cwd=/tmp/世;cmdline=echo\\x3b ok;",
        "vm=test;container=box;targetuser=root;targethost=remote;sessionid=last\x1b\\"
    );
    for byte in body.as_bytes() {
        term.advance(&[*byte]);
    }
    let context = &term.contexts()[0];
    assert_eq!(context.id, "a;b\\c");
    assert_eq!(context.fields.len(), 15); // More fields than vte's callback table retains.
    assert_eq!(context.get(FieldKey::SessionId), Some("last"));
    assert_eq!(context.get(FieldKey::Cwd), Some("/tmp/世"));
    assert_eq!(context.get(FieldKey::Cmdline), Some("echo; ok"));
    assert_eq!(context.get(FieldKey::Pid), Some("18446744073709551615"));
}

#[test]
fn malformed_fields_are_independent_and_ids_are_not_repaired() {
    let mut term = Terminal::new(20, 4, 0, 0);
    osc(
        &mut term,
        "start=ok;user=good;user=bad\\x00;unknown=ignored;pid=+4;pidfdid=18446744073709551616;type=wrong;hostname=host;cmdline=",
    );
    let context = &term.contexts()[0];
    assert_eq!(context.get(FieldKey::User), Some("good"));
    assert_eq!(context.get(FieldKey::Hostname), Some("host"));
    assert_eq!(context.get(FieldKey::Pid), None);
    assert_eq!(context.get(FieldKey::PidFdId), None);
    assert_eq!(context.get(FieldKey::Type), None);
    assert_eq!(context.get(FieldKey::Cmdline), Some(""));
    for invalid in [
        "start=",
        "end=",
        "start=bad\\oops",
        "start=世",
        "type=shell;start=wrong",
        "start=a\0b",
    ] {
        osc(&mut term, invalid);
        assert_eq!(term.contexts().len(), 1);
    }
    // A C0 inside a single field is discarded by vte, but our raw validation sees it.
    osc(&mut term, "start=ok;user=bad\0name;hostname=valid");
    assert_eq!(term.contexts()[0].get(FieldKey::User), None);
    assert_eq!(term.contexts()[0].get(FieldKey::Hostname), Some("valid"));
    osc(&mut term, &format!("start={}", "x".repeat(65)));
    osc(
        &mut term,
        &format!("start=ok;user={};hostname=kept", "x".repeat(256)),
    );
    assert_eq!(term.contexts()[0].get(FieldKey::User), None);
    assert_eq!(term.contexts()[0].get(FieldKey::Hostname), Some("kept"));
    term.advance(b"AFTER\x1b[5n");
    assert!(super::row(&term, 0).starts_with("AFTER"));
    assert_eq!(term.replies, b"\x1b[0n");
}

#[test]
fn overflow_keeps_earlier_contexts_updates_and_end_still_work() {
    let mut term = Terminal::new(20, 4, 0, 0);
    for index in 0..MAX_DEPTH {
        osc(&mut term, &format!("start={index}"));
    }
    let before = term.contexts().to_vec();
    osc(&mut term, "start=overflow;type=elevate");
    assert_eq!(term.contexts(), before);
    osc(&mut term, "start=0;user=updated");
    assert_eq!(term.contexts().len(), 1);
    assert_eq!(term.contexts()[0].get(FieldKey::User), Some("updated"));
    assert!(term.clear_contexts());
    assert!(!term.clear_contexts());
    assert!(term.contexts().is_empty());
}

#[test]
fn truncated_context_is_rejected_without_swallowing_output() {
    let mut term = Terminal::new(20, 4, 0, 0);
    osc(&mut term, "start=kept");
    osc(
        &mut term,
        &format!("start=kept;user={}", "x".repeat(70_000)),
    );
    assert_eq!(term.contexts()[0].id, "kept");
    assert!(term.contexts()[0].fields.is_empty());
    term.advance(b"Z");
    assert!(super::row(&term, 0).starts_with('Z'));
}

#[test]
fn cancellation_keeps_contexts_and_following_output() {
    // CAN/SUB abort an incomplete start; they do not commit its valid prefix.
    for cancel in [0x18, 0x1a] {
        let mut term = Terminal::new(20, 4, 0, 0);
        osc(&mut term, "start=kept");
        term.advance(b"\x1b]3008;start=aborted;user=test");
        term.advance(&[cancel]);
        term.advance(b"AFTER\x1b[5n");
        assert_eq!(term.contexts().len(), 1);
        assert_eq!(term.contexts()[0].id, "kept");
        assert!(super::row(&term, 0).starts_with("AFTER"));
        assert_eq!(term.replies, b"\x1b[0n");
    }
}

#[test]
fn unused_contexts_have_zero_allocations_or_events_on_plain_output() {
    let mut term = Terminal::new(20, 4, 0, 0);
    term.advance(b"warm\r");
    let measured = allocation_counter::measure(|| {
        for _ in 0..1000 {
            term.advance(b"ordinary\r");
        }
    });
    assert_eq!(measured.count_total, 0);
    assert!(term.contexts().is_empty());
    assert!(term.events.is_empty());
    assert!(puzzle_vt::osc::is_builtin(3008));
}
