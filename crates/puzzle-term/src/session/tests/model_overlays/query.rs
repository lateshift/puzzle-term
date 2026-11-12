//! Search input mode consumes accepted keys, IME and clipboard text before PTY encoding.
use super::*;

#[test]
fn native_query_consumes_keys_and_ime_and_restores_input() {
    let (mut session, writes, _) = fixture();
    let now = Instant::now();
    session.feed("漢字 漢字".as_bytes(), now);
    input(&mut session, Command::Search(SearchCommand::Request), now);
    key(&mut session, Key::Char('漢'), now);
    input(
        &mut session,
        Command::Preedit(crate::messages::PreeditState {
            text: "じ".into(),
            cursor: Some(3),
        }),
        now,
    );
    assert_eq!(
        session.search_bar_query(),
        Some("漢"),
        "preedit never searches"
    );
    input(&mut session, Command::Text("字".into()), now);
    assert_eq!(session.search_bar_query(), Some("漢字"));
    session.publish(now);
    assert_eq!(session.search_state().count, 2);
    key(&mut session, Key::Enter, now);
    assert_eq!(session.search_state().current, Some(0));
    key(&mut session, Key::Enter, now);
    assert_eq!(session.search_state().current, Some(1));
    let mut previous = KeyEvent::press(Key::Enter);
    previous.modifiers.0 = Modifiers::SHIFT;
    input(&mut session, Command::Key(previous), now);
    assert_eq!(session.search_state().current, Some(0));
    // Moving inside multibyte text and deleting must preserve valid UTF-8 boundaries.
    key(&mut session, Key::Left, now);
    key(&mut session, Key::Backspace, now);
    assert_eq!(session.search_bar_query(), Some("字"));
    key(&mut session, Key::Delete, now);
    assert_eq!(session.search_bar_query(), Some(""));
    input(&mut session, Command::Paste("x\n\x1by".into()), now);
    assert_eq!(session.search_bar_query(), Some("xy"));
    // A modifier chord, release and composing key must never reach the application.
    let mut chord = KeyEvent::press(Key::Char('c'));
    chord.modifiers.0 = Modifiers::CTRL;
    input(&mut session, Command::Key(chord.clone()), now);
    chord.phase = crate::vt::input::KeyPhase::Release;
    input(&mut session, Command::Key(chord), now);
    assert!(writes.lock().expect("writes").is_empty());
    key(&mut session, Key::Escape, now);
    assert_eq!(session.search_bar_query(), None);
    assert_eq!(session.search_state().count, 0);
    key(&mut session, Key::Char('z'), now);
    assert_eq!(writes.lock().expect("writes").concat(), b"z");
}

#[test]
fn reopened_search_restores_retained_query_and_same_line_current_counter() {
    let (mut session, _, _) = fixture();
    let mut context = super::overlays::text_context(1.0);
    session.set_text_context(&context);
    let now = Instant::now();
    session.install_model_overlays();
    session.feed(b"hit hit", now);
    input(&mut session, Command::Search(SearchCommand::Request), now);
    input(&mut session, Command::Text("hit".into()), now);
    session.draw(now, &mut context).expect("search");
    key(&mut session, Key::Enter, now);
    session.draw(now, &mut context).expect("first");
    key(&mut session, Key::Enter, now);
    session.draw(now, &mut context).expect("second");
    assert_eq!(session.snapshot.overlays.models.search.current, Some(1));
    key(&mut session, Key::Escape, now);
    assert_eq!(session.search_state().count, 0);
    input(&mut session, Command::Search(SearchCommand::Request), now);
    session.draw(now, &mut context).expect("reopened");
    assert_eq!(session.search_bar_query(), Some("hit"));
    assert_eq!(session.search_state().count, 2);
}

#[test]
fn native_query_keeps_configured_clipboard_paste_without_pty_bytes() {
    use crate::keybind::{Keybindings, Platform, Source};
    let (mut session, writes, _) = fixture();
    let now = Instant::now();
    let mut bindings = Keybindings::default();
    assert!(
        bindings
            .apply(
                &["super+v=paste".into(), "super+k=clearScreen".into()],
                Source::User,
                Platform::Mac,
                false
            )
            .is_empty()
    );
    session.set_keybindings(bindings);
    session.services.clipboard = Box::new(crate::platform::MemoryClipboard::default());
    session
        .services
        .clipboard
        .write_text("hit\n漢")
        .expect("clipboard");
    input(&mut session, Command::Search(SearchCommand::Request), now);
    let mut paste = KeyEvent::press(Key::Char('v'));
    paste.modifiers.0 = Modifiers::SUPER;
    input(&mut session, Command::Key(paste), now);
    assert_eq!(session.search_bar_query(), Some("hit漢"));
    // Other configured app actions remain suppressed while the query owns input.
    session.feed(b"keep", now);
    let mut clear = KeyEvent::press(Key::Char('k'));
    clear.modifiers.0 = Modifiers::SUPER;
    input(&mut session, Command::Key(clear), now);
    assert_eq!(
        session
            .buffer()
            .line(session.buffer().cursor().expect("cursor").line)
            .expect("row")
            .cell(0)
            .expect("cell")
            .raw
            .ch,
        u32::from('k')
    );
    assert!(writes.lock().expect("writes").is_empty());
}

#[test]
fn presented_search_buttons_navigate_and_close_through_model_view_split() {
    for dpr in [1.0, 2.0] {
        let (mut session, _, _) = fixture();
        let mut context = super::overlays::text_context(dpr);
        session.set_text_context(&context);
        session.install_model_overlays();
        let now = Instant::now();
        session.feed(b"hit hit", now);
        input(&mut session, Command::Search(SearchCommand::Request), now);
        input(&mut session, Command::Text("hit".into()), now);
        let frame = session.draw(now, &mut context).expect("search frame");
        let width = frame.target_size[0] as f32 / dpr;
        let panel_width = width.min(420.0);
        let panel_x = (width - 432.0).max(0.0);
        session.presented();
        // Next twice and Prev once select the first same-row occurrence again.
        for button in [1, 1, 0] {
            let point = [
                panel_x + panel_width - 168.0 + button as f32 * 54.0 + 25.0,
                54.0,
            ];
            pointer(&mut session, PointerKind::Press, point, now);
            pointer(&mut session, PointerKind::Release, point, now);
        }
        assert_eq!(session.search_state().current, Some(0));
        let close = [panel_x + panel_width - 35.0, 54.0];
        pointer(&mut session, PointerKind::Press, close, now);
        pointer(&mut session, PointerKind::Release, close, now);
        assert_eq!(session.search_bar_query(), None);
        assert_eq!(session.search_state().count, 0);
    }
}
