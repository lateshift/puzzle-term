//! Public Session search contracts protect progress, live results and navigation.
//!
//! History is produced through parser bytes. No private query fields or hand-made
//! matches choose the expected counts or centres. Frames are clock-injected and
//! assertions count physical rows, so a hidden whole-history scan cannot pass.
use super::*;
use crate::{search::SearchCommand, viewport::ScrollTarget, vt::search::FindOptions};

/// Starts a cold query through public policy without advancing it during pump.
fn find(session: &mut TestHost, query: &str) {
    session.submit(
        Command::Search(SearchCommand::Find {
            query: query.into(),
            options: FindOptions::default(),
        }),
        Origin::Api,
    );
    session.pump(Instant::now());
}
/// Drives bounded work to completion, failing if a source never converges.
fn finish(session: &mut TestHost) -> usize {
    let mut frames = 0;
    while !session.search_state().complete {
        session.publish(Instant::now());
        assert!(session.search_state().scanned <= 2_000);
        frames += 1;
        assert!(frames < 200, "search must converge");
    }
    frames
}
/// A 100,000-line history must yield between physical-row slices (§6.10).
#[test]
fn hundred_thousand_lines_are_sliced() {
    let (_, writes, wakes) = fixture();
    let services = Services::new(
        Box::new(RecordingTransport(writes)),
        Box::new(ManualWaker(wakes.clone())),
    );
    let config = SessionConfig {
        columns: 12,
        rows: 1,
        scrollback_lines: 100_000,
        // This explicit large-history fixture must fit 100,000 boxed row headers
        // as well as cells. The production 32-MiB default remains unchanged.
        scrollback_bytes: 64 * 1024 * 1024,
        ..SessionConfig::default()
    };
    let mut session = TestHost::new(config, services);
    let now = Instant::now();
    for row in 0..100_000 {
        session.feed(
            if row % 100 == 0 {
                b"hit\r\n"
            } else {
                b"empty\r\n"
            },
            now,
        );
    }
    assert_eq!(session.terminal().screen().ring.scrollback_len(), 100_000);
    find(&mut session, "hit");
    assert_eq!(session.search_state().count, 0, "pump never scans");
    assert!(finish(&mut session) >= 51);
    assert_eq!(session.search_state().count, 1_000);
    assert_eq!(session.search_fills().len(), 1_000);
    let before = wakes.load(Ordering::Relaxed);
    assert!(
        session.publish(now).is_none(),
        "finished search leaves no idle frame"
    );
    assert_eq!(wakes.load(Ordering::Relaxed), before);
}
/// Smart case joins soft wraps but not hard newlines and preserves whole wide cells.
#[test]
fn smart_case_wraps_and_wide_cells() {
    let (mut session, _, _) = fixture();
    session.terminal.resize(4, 4);
    session.feed("abcdEf\r\n界Q".as_bytes(), Instant::now());
    find(&mut session, "de");
    finish(&mut session);
    assert_eq!(session.search_state().count, 1);
    let range = session.search_fills()[0].range;
    assert_eq!((range.start.column, range.end.column), (3, 0));
    assert_ne!(range.start.line, range.end.line);
    find(&mut session, "DE");
    finish(&mut session);
    assert_eq!(session.search_state().count, 0);
    find(&mut session, "界");
    finish(&mut session);
    let range = session.search_fills()[0].range;
    assert_eq!((range.start.column, range.end.column), (0, 1));
    find(&mut session, "f界");
    finish(&mut session);
    assert_eq!(session.search_state().count, 0, "hard rows cannot join");
}
/// Append, eviction and rewrite must neither duplicate hits nor navigate gone cells.
#[test]
fn output_eviction_and_rewrite_reconcile_matches() {
    let (_, writes, wakes) = fixture();
    let services = Services::new(
        Box::new(RecordingTransport(writes)),
        Box::new(ManualWaker(wakes)),
    );
    let mut session = TestHost::new(
        SessionConfig {
            columns: 8,
            rows: 2,
            scrollback_lines: 3,
            ..SessionConfig::default()
        },
        services,
    );
    let now = Instant::now();
    session.feed(b"hit\r\nhit", now);
    find(&mut session, "hit");
    finish(&mut session);
    let first = session.search_fills()[0].range;
    session.feed(b"\r\nhit\r\n", now);
    session.publish(now);
    finish(&mut session);
    assert_eq!(session.search_state().count, 3);
    assert_eq!(session.search_fills()[0].range, first);
    // Moving an unchanged old grid row into history preserves its tracked range.
    session.feed(b"empty\r\nempty\r\nempty\r\n", now);
    session.publish(now);
    finish(&mut session);
    assert_eq!(session.search_state().count, 1);
    assert_ne!(session.search_fills()[0].range, first);
    session.feed(b"\x1b[1;1Hhit", now);
    session.publish(now);
    finish(&mut session);
    assert_eq!(session.search_state().count, 2);
    session.feed(b"\x1b[1;1Hgone", now);
    session.publish(now);
    finish(&mut session);
    assert_eq!(session.search_state().count, 1);
    session.feed(b"\x1b[?1049h", now);
    session.publish(now);
    finish(&mut session);
    assert_eq!(session.search_state().count, 0);
}
/// Next/prev follows the current hit even when several hits share the same row.
#[test]
fn navigation_centres_and_updates_markers() {
    let (mut session, _, _) = fixture();
    session.terminal.resize(12, 5);
    let now = Instant::now();
    for row in 0..30 {
        session.feed(
            if row == 10 || row == 20 {
                b"hit hit\r\n"
            } else {
                b"empty\r\n"
            },
            now,
        );
    }
    find(&mut session, "hit");
    finish(&mut session);
    assert_eq!(session.search_state().count, 4);
    for (delta, expected) in [(1, 1), (1, 2), (1, 3), (-1, 2), (-2, 0)] {
        session.submit(Command::ScrollTo(ScrollTarget::Match(delta)), Origin::Api);
        session.pump(now);
        assert_eq!(session.search_state().current, Some(expected));
        let range = session.search_fills()[expected].range;
        let index = session
            .terminal()
            .screen()
            .ring
            .index_of(range.start.line)
            .expect("retained");
        assert_eq!(session.viewport().top_index(), index - 2);
        assert!(session.search_fills()[expected].current);
        assert_eq!(
            session.search_fills().iter().filter(|f| f.current).count(),
            1
        );
    }
    assert!(!session.viewport().scrollbar.markers().is_empty());
    session.submit(Command::Search(SearchCommand::Clear), Origin::Api);
    session.pump(now);
    assert_eq!(session.search_state().count, 0);
    assert!(session.search_fills().is_empty());
}

/// A same-turn find/step survives the first slice without host-side intent storage.
#[test]
fn early_navigation_waits_for_matches_and_keeps_latest_direction() {
    for (steps, replacement, expected) in [
        (vec![1], None, Some(0)),
        (vec![-1], None, Some(1)),
        (vec![1, -1], None, Some(1)),
        (vec![-1, 1], None, Some(0)),
        (vec![1], Some("absent"), None),
        (vec![-1], Some(""), None),
    ] {
        let (mut session, _, _) = fixture();
        session.terminal.resize(12, 5);
        let now = Instant::now();
        session.feed(
            b"empty\r\nempty\r\nhit\r\nhit\r\nhit\r\nempty\r\nempty\r\n",
            now,
        );
        // Neither the query nor the step gets a search slice during this drain.
        session.submit(
            Command::Search(SearchCommand::Find {
                query: "hit".into(),
                options: FindOptions::default(),
            }),
            Origin::Api,
        );
        for step in steps {
            session.submit(Command::ScrollTo(ScrollTarget::Match(step)), Origin::Api);
        }
        if let Some(query) = replacement {
            session.submit(
                Command::Search(if query.is_empty() {
                    SearchCommand::Clear
                } else {
                    SearchCommand::Find {
                        query: query.into(),
                        options: FindOptions::default(),
                    }
                }),
                Origin::Api,
            );
        }
        session.pump(now);
        assert_eq!(session.search_state().current, None);
        assert_eq!(session.search_state().count, 0);
        session.publish(now);
        assert_eq!(session.search_state().current, expected);
        if let Some(index) = expected {
            let range = session.search_fills()[index].range;
            let ring = &session.terminal().screen().ring;
            let row = ring.index_of(range.start.line).expect("retained");
            assert_eq!(session.viewport().top_index(), row - 2);
            assert!(session.search_fills()[index].current);
            // Consuming the pending step is one-shot, not another navigation each frame.
            session.publish(now);
            assert_eq!(session.search_state().current, expected);
        }
    }
}

/// Records the coarse UI request without any frame observer allocation.
struct Requests(Arc<AtomicUsize>);
impl Observer<Command, Event> for Requests {
    fn observe(&mut self, event: &Event, _: &mut HookCtx<'_, Command, Event>) {
        if matches!(event, Event::SearchRequested) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Both hosts receive find intent; native also opens its default query input mode.
#[test]
fn search_actions_follow_platform_and_result_availability() {
    use crate::keybind::{Keybindings, Platform, Source};
    for web in [false, true] {
        let (mut session, writes, _) = fixture();
        let mut table = Keybindings::default();
        let issues = table.apply(
            &[
                "f=search".into(),
                "performable:n=searchNext".into(),
                "performable:p=searchPrev".into(),
            ],
            Source::User,
            Platform::Mac,
            web,
        );
        assert!(issues.is_empty());
        session.set_keybindings(table);
        let requested = Arc::new(AtomicUsize::new(0));
        session
            .bus_mut()
            .observe(
                EventKinds::at(26),
                0,
                "search-test",
                Requests(requested.clone()),
            )
            .expect("observer");
        for ch in ['f', 'n', 'p'] {
            session.submit(Command::Key(KeyEvent::press(Key::Char(ch))), Origin::User);
            session.pump(Instant::now());
        }
        assert_eq!(requested.load(Ordering::Relaxed), 1);
        assert_eq!(
            writes.lock().expect("writes").concat(),
            if web {
                b"np".as_slice()
            } else {
                b"".as_slice()
            }
        );
        if !web {
            assert_eq!(session.search_bar_query(), Some("np"));
            session.submit(Command::Key(KeyEvent::press(Key::Escape)), Origin::User);
            session.pump(Instant::now());
            assert_eq!(session.search_bar_query(), None);
        }
        writes.lock().expect("writes").clear();
        session.feed(b"hit hit", Instant::now());
        find(&mut session, "hit");
        finish(&mut session);
        for (ch, current) in [('n', 0), ('n', 1), ('p', 0)] {
            session.submit(Command::Key(KeyEvent::press(Key::Char(ch))), Origin::User);
            session.pump(Instant::now());
            assert_eq!(session.search_state().current, Some(current));
        }
        assert!(writes.lock().expect("writes").is_empty());
    }
}

/// Native host UI and nonblocking selection reads use the public message seam.
mod host_ui;

/// Initial selection and delayed slices share the native and built-in model.
mod automatic;
