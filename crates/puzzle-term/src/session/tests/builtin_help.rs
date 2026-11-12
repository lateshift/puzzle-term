//! Shortcut overlays share matcher state and Session's Unicode query mode (§11.6).
use super::*;
use crate::keybind::{Keybindings, Platform, Source};

/// Prepare a source-layered table without unrelated platform defaults.
fn table() -> Keybindings {
    let mut table = Keybindings::default();
    assert!(
        table
            .apply(
                &[
                    "a>c=copy".into(),
                    "a>d=sendText:hello".into(),
                    "b>e=ignore".into()
                ],
                Source::Defaults,
                Platform::Mac,
                false
            )
            .is_empty()
    );
    assert!(
        table
            .apply(&["x=search".into()], Source::User, Platform::Mac, false)
            .is_empty()
    );
    assert!(
        table
            .apply(&["z=zoom:2".into()], Source::Profile, Platform::Mac, false)
            .is_empty()
    );
    assert!(
        table
            .apply(
                &["h=showKeybindings".into()],
                Source::Session,
                Platform::Mac,
                false
            )
            .is_empty()
    );
    table
}

/// Releases do not count as grammar steps, and no unrelated prefix is advertised.
#[test]
fn leader_hint_lists_exact_continuations_and_disappears_on_completion() {
    let (mut session, writes, _) = fixture();
    let mut context = super::overlays::text_context(1.0);
    let now = Instant::now();
    session.set_text_context(&context);
    session.install_model_overlays();
    session.set_keybindings(table());
    let press = KeyEvent::press(Key::Char('a'));
    session.submit(Command::Key(press.clone()), Origin::User);
    session.pump(now);
    let mut release = press;
    release.phase = crate::vt::input::KeyPhase::Release;
    session.submit(Command::Key(release), Origin::User);
    session.pump(now);
    session.draw(now, &mut context).expect("leader frame");
    assert_eq!(
        session.snapshot.overlays.models.help.leader,
        "c: copy:text · d: sendText:hello"
    );
    assert!(writes.lock().expect("writes").is_empty());
    session.submit(Command::Key(KeyEvent::press(Key::Char('d'))), Origin::User);
    session.pump(now);
    session.draw(now, &mut context).expect("clear leader");
    assert!(session.snapshot.overlays.models.help.leader.is_empty());
    assert_eq!(writes.lock().expect("writes").concat(), b"hello");
}

/// Filtering, composition and Escape remain local and preserve the text search model.
#[test]
fn cheat_sheet_shows_sources_and_owns_only_its_filter_input() {
    for dpr in [1.0, 2.0] {
        let (mut session, writes, _) = fixture();
        let mut context = super::overlays::text_context(dpr);
        let now = Instant::now();
        session.set_text_context(&context);
        session.install_model_overlays();
        session.set_keybindings(table());
        session.submit(Command::Key(KeyEvent::press(Key::Char('h'))), Origin::User);
        session.pump(now);
        session.draw(now, &mut context).expect("cheat sheet");
        let help = &session.snapshot.overlays.models.help;
        assert!(help.open);
        assert!(help.rows.iter().any(|row| row.source == "defaults"));
        assert!(help.rows.iter().any(|row| row.source == "user"));
        assert!(help.rows.iter().any(|row| row.source == "profile"));
        assert!(help.rows.iter().any(|row| row.source == "session"));
        // Provisional IME stays at the field caret and never filters or writes input.
        session.submit(
            Command::Preedit(crate::messages::PreeditState {
                text: "候補".into(),
                cursor: Some(2),
            }),
            Origin::User,
        );
        session.pump(now);
        session.draw(now, &mut context).expect("filter composition");
        assert_eq!(session.snapshot.overlays.models.help.preedit, "候補");
        assert!(session.snapshot.overlays.models.help.filter.is_empty());
        // Typing a bound key edits the filter instead of invoking its zoom action.
        session.submit(Command::Text("zoom".into()), Origin::User);
        session.pump(now);
        session.draw(now, &mut context).expect("filtered");
        let help = &session.snapshot.overlays.models.help;
        assert_eq!(help.rows.len(), 1);
        assert_eq!(help.rows[0].action, "zoom:2");
        assert_eq!(help.rows[0].group, "Appearance and settings");
        assert!(writes.lock().expect("writes").is_empty());
        assert_eq!(session.search_state().count, 0);
        session.submit(Command::Key(KeyEvent::press(Key::Escape)), Origin::User);
        session.pump(now);
        session.draw(now, &mut context).expect("closed");
        assert!(!session.snapshot.overlays.models.help.open);
        session.submit(Command::Text("normal".into()), Origin::User);
        session.pump(now);
        assert_eq!(writes.lock().expect("writes").concat(), b"normal");
    }
}
