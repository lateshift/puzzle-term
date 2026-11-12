//! Automatic current selection must be stable, visible and independent of Enter timing.
use super::*;

#[test]
fn initial_current_prefers_newest_visible_match_without_scrolling() {
    let (mut session, _, _) = fixture();
    session.terminal.resize(12, 5);
    let now = Instant::now();
    session.feed(b"hit\r\nempty\r\nhit\r\nempty\r\nhit hit\r\nempty\r\n", now);
    let top = session.viewport().top_index();
    find(&mut session, "hit");
    finish(&mut session);
    assert_eq!(session.search_state().current, Some(3));
    assert_eq!(session.viewport().top_index(), top);
    assert!(session.search_fills()[3].current);
}

#[test]
fn initial_offscreen_match_centres_once_and_later_slices_preserve_it() {
    let (mut session, _, _) = fixture();
    session.terminal.resize(12, 5);
    let now = Instant::now();
    for row in 0..2500 {
        session.feed(
            if row == 10 || row == 2490 {
                b"hit\r\n"
            } else {
                b"empty\r\n"
            },
            now,
        );
    }
    find(&mut session, "hit");
    session.publish(now);
    assert_eq!(session.search_state().count, 1);
    assert_eq!(session.search_state().current, Some(0));
    assert_eq!(session.viewport().top_index(), 8);
    assert!(!session.search_state().complete);
    finish(&mut session);
    assert_eq!(session.search_state().count, 2);
    assert_eq!(session.search_state().current, Some(0));
    assert_eq!(session.viewport().top_index(), 8);
}

#[test]
fn immediate_enter_equals_enter_after_automatic_selection() {
    for early in [true, false] {
        let (mut session, _, _) = fixture();
        session.feed(b"hit hit hit", Instant::now());
        find(&mut session, "hit");
        if !early {
            finish(&mut session);
        }
        session.submit(Command::ScrollTo(ScrollTarget::Match(1)), Origin::Api);
        session.pump(Instant::now());
        finish(&mut session);
        assert_eq!(session.search_state().current, Some(0));
        // A replacement starts from its own newest visible match, not the old index.
        find(&mut session, "hit");
        finish(&mut session);
        assert_eq!(session.search_state().current, Some(2));
        session.submit(Command::Search(SearchCommand::Clear), Origin::Api);
        session.pump(Instant::now());
        assert_eq!(session.search_state().current, None);
    }
}
