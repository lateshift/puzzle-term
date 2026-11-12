//! Functional activation boundaries, independent of scheduler speed or GPU state.
#![allow(
    clippy::expect_used,
    reason = "Missing fixture rows or evidence files must fail these regression assertions."
)]
use crate::{
    Terminal, print_work,
    selection::{SelectPoint, Selection, SelectionKind},
};

/// Rebase includes history, inactive documents and reusable rows without repainting them.
#[test]
fn activation_exceeds_every_existing_version_without_changing_cache_keys() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    term.advance(&b"history\r\n".repeat(20));
    term.advance(b"\x1b[3J\x1b[?47h");
    term.advance(&b"\rX".repeat(200));
    term.advance(b"\x1b[?47l");
    let maximum = term
        .primary
        .ring
        .maximum_write_version()
        .max(term.alternate.ring.maximum_write_version());
    let before: Vec<_> = [&term.primary, &term.alternate]
        .into_iter()
        .flat_map(|screen| screen.ring.iter().map(|row| (row.id, row.version)))
        .collect();
    assert_eq!(term.write_clock(), 0);
    term.register_source_observer();
    assert!(term.write_clock() > maximum);
    let after: Vec<_> = [&term.primary, &term.alternate]
        .into_iter()
        .flat_map(|screen| screen.ring.iter().map(|row| (row.id, row.version)))
        .collect();
    assert_eq!(before, after, "activation must not invalidate row caches");
    term.advance(b"\rY");
    assert!(
        term.screen()
            .row(term.cursor.row)
            .expect("required regression fixture")
            .version
            > maximum
    );
    term.unregister_source_observer();
    let paused = term.write_clock();
    let work = print_work::measure(|| term.advance(b"\rZ"));
    assert_eq!(work[6], 0);
    assert_eq!(term.write_clock(), paused);
}

/// A surviving owner or watch keeps global order; the last release pauses it.
#[test]
fn watches_keep_clock_live_until_last_release_and_reactivation_rebases() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    term.advance(b"abcdefgh");
    let line = term.cursor_line().expect("required regression fixture");
    let first = term
        .watch_source((line, 2), (line, 4), 4)
        .expect("required regression fixture");
    term.register_source_observer();
    term.unregister_source_observer();
    let active = print_work::measure(|| term.advance(b"\rX"));
    assert_eq!(active[6], 1);
    assert!(
        !term
            .source_watch(first)
            .expect("required regression fixture")
            .changed
    );
    term.release_source_watch(first);
    let paused = term.write_clock();
    let idle = print_work::measure(|| term.advance(&b"\rX".repeat(100)));
    assert_eq!(idle[6], 0);
    assert_eq!(term.write_clock(), paused);
    let high = term
        .screen()
        .row(0)
        .expect("required regression fixture")
        .version;
    let second = term
        .watch_source((line, 2), (line, 4), 4)
        .expect("required regression fixture");
    assert!(
        term.source_watch(second)
            .expect("required regression fixture")
            .clock
            > high
    );
    term.resize(5, 4);
    term.resize(12, 4);
    assert!(
        !term
            .source_watch(second)
            .expect("required regression fixture")
            .changed
    );
    term.advance(b"\x1b[1;3Hc");
    assert!(
        term.source_watch(second)
            .expect("required regression fixture")
            .changed,
        "identical rewrite invalidates"
    );
}

/// Selection is a source consumer too: identical rewrites after reflow remain invalid.
#[test]
fn selection_activates_before_capture_and_releases_its_observation_once() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    term.advance(b"abcdefgh");
    let line = term.cursor_line().expect("required regression fixture");
    let selection = Selection::new(
        &mut term,
        SelectionKind::Simple,
        SelectPoint { line, column: 2 },
        SelectPoint { line, column: 5 },
    )
    .expect("required regression fixture");
    term.resize(5, 4);
    term.resize(12, 4);
    assert_eq!(selection.text(&term, " ").as_deref(), Some("cdef"));
    term.advance(b"\x1b[1;3Hc");
    assert!(selection.text(&term, " ").is_none());
    let copied = selection.clone();
    selection.clear(&mut term);
    copied.clear(&mut term);
    let work = print_work::measure(|| term.advance(b"\rZ"));
    assert_eq!(work[6], 0);
}

/// Multiple registered consumers cannot disable one another through reset or release.
#[test]
fn registration_lifetime_survives_ris_and_balances_multiple_consumers() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    term.register_source_observer();
    term.register_source_observer();
    term.unregister_source_observer();
    let previous = term.write_clock();
    term.advance(b"\x1bcX");
    assert!(term.write_clock() > previous);
    term.unregister_source_observer();
    let work = print_work::measure(|| term.advance(b"\rX"));
    assert_eq!(work[6], 0);
}

/// A rejected request has no source lifetime and cannot leave stamping enabled.
#[test]
fn refused_watch_does_not_activate_clock() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    term.advance(b"text");
    let line = term.cursor_line().expect("required regression fixture");
    assert!(term.watch_source((line, 0), (line, 4), 0).is_none());
    assert_eq!(term.write_clock(), 0);
    let work = print_work::measure(|| term.advance(b"X"));
    assert_eq!(work[6], 0);
}
