//! Native UI ownership must not borrow the browser's security/platform identity.
use super::*;
use crate::{HostEvent, SessionMessage, search::SearchUi};

#[test]
fn host_find_contract_does_not_capture_text_or_change_keybinding_platform() {
    let (mut session, writes, _) = fixture();
    session.enable_host_events().expect("events");
    let now = Instant::now();
    session
        .apply(SessionMessage::SearchUi(SearchUi::Host), now)
        .expect("ui");
    // Reloading the shortcut table must not restore the built-in UI implicitly.
    let mut bindings = crate::keybind::Keybindings::default();
    bindings.apply(
        &["f=search".into()],
        crate::keybind::Source::User,
        crate::keybind::Platform::Mac,
        false,
    );
    session.set_keybindings(bindings);
    session
        .apply(
            SessionMessage::NativeKey(KeyEvent::press(Key::Char('f')), Origin::User),
            now,
        )
        .expect("find");
    assert_eq!(session.search_bar_query(), None);
    let mut requested = 0;
    while let Some(event) = session.take_event() {
        requested += usize::from(matches!(event, HostEvent::SearchRequested));
    }
    assert_eq!(requested, 1);
    session.submit(Command::Text("x".into()), Origin::User);
    session.pump(now);
    assert_eq!(writes.lock().expect("writes").concat(), b"x");
    session
        .apply(SessionMessage::SelectedText(17), now)
        .expect("text");
    let mut selected = None;
    while let Some(event) = session.take_event() {
        if let HostEvent::SelectedText { request, text } = event {
            selected = Some((request, text));
        }
    }
    assert_eq!(selected, Some((17, String::new())));
}

#[test]
fn replacing_open_builtin_query_releases_input_but_preserves_matches() {
    let (mut session, _, _) = fixture();
    let now = Instant::now();
    session.feed(b"hit hit", now);
    session.submit(Command::Search(SearchCommand::Request), Origin::Api);
    session.pump(now);
    assert!(session.search_bar_query().is_some());
    find(&mut session, "hit");
    finish(&mut session);
    assert_eq!(session.search_state().count, 2);
    session
        .apply(SessionMessage::SearchUi(SearchUi::Host), now)
        .expect("host");
    assert_eq!(session.search_bar_query(), None);
    assert_eq!(session.search_state().count, 2);
    session
        .apply(SessionMessage::SearchUi(SearchUi::BuiltIn), now)
        .expect("builtin");
    assert_eq!(session.search_bar_query(), None);
}
