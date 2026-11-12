//! Public keyboard navigation over a neutral recorded OSC 133 byte stream.
use super::*;
use crate::{
    keybind::{Action, Keybindings, Platform, Source, defaults},
    viewport::ScrollTarget,
    vt::{input::Modifiers, zones::ZoneKind},
};
/// Replays the authored bridge recording without bypassing parsing or bus facts.
fn recorded() -> (TestHost, Writes) {
    let (mut session, writes, _) = fixture();
    let recording = include_bytes!("../../../../../assets/fixtures/scrolling/prompts.vtrec");
    let length = u32::from_le_bytes(recording[4..8].try_into().expect("record length")) as usize;
    assert_eq!(recording.len(), 8 + length);
    session.feed(&recording[8..], Instant::now());
    (session, writes)
}
/// Sends a shortcut through the built-in interceptor, without building a frame.
fn press(session: &mut TestHost, key: Key, modifiers: u8) {
    let mut event = KeyEvent::press(key);
    event.modifiers = Modifiers(modifiers);
    session.submit(Command::Key(event), Origin::User);
    session.pump(Instant::now());
}
/// Platform defaults use one page minus a context row and explicit boundaries.
#[test]
fn keyboard_scroll_actions_and_defaults() {
    for platform in [Platform::Mac, Platform::Windows, Platform::Linux] {
        let (mut session, writes) = recorded();
        let mut table = Keybindings::default();
        let issues = table.apply(
            &defaults::lines(platform),
            Source::Defaults,
            platform,
            false,
        );
        // Later overlays/selection can still be unavailable; scrolling cannot be.
        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("scroll") || issue.contains("jumpToPrompt"))
        );
        session.set_keybindings(table);
        press(&mut session, Key::PageUp, Modifiers::SHIFT);
        assert_eq!(session.viewport().offset_rows(), 23);
        press(&mut session, Key::PageDown, Modifiers::SHIFT);
        assert!(session.viewport().following());
        let boundary = if platform == Platform::Mac {
            Modifiers::SUPER
        } else {
            Modifiers::SHIFT
        };
        press(&mut session, Key::Home, boundary);
        assert_eq!(session.viewport().top_index(), 0);
        press(&mut session, Key::End, boundary);
        assert!(session.viewport().following());
        assert!(writes.lock().expect("writes").is_empty());
    }
    let (mut session, _) = recorded();
    // Argument-bearing actions reuse the same public relative-navigation lane.
    for (action, expected) in [
        (Action::ScrollLines(-5), 5),
        (Action::ScrollPages(-0.5), 16),
    ] {
        session.submit(Command::BindingAction { action, all: false }, Origin::User);
        session.pump(Instant::now());
        assert_eq!(session.viewport().offset_rows(), expected);
    }
}
/// Repeated prompt jumps must name distinct retained OSC 133 boundaries.
#[test]
fn recorded_prompt_jumps() {
    let (mut session, writes) = recorded();
    let prompts = session
        .buffer()
        .zones()
        .filter(|zone| zone.kind == ZoneKind::Prompt)
        .map(|zone| zone.line)
        .collect::<Vec<_>>();
    assert_eq!(prompts.len(), 8);
    let mut table = Keybindings::default();
    table.apply(
        &defaults::lines(Platform::Mac),
        Source::Defaults,
        Platform::Mac,
        false,
    );
    session.set_keybindings(table);
    // The live grid begins between the fifth and sixth retained prompt marks.
    for expected in [prompts[5], prompts[4]] {
        press(&mut session, Key::Up, Modifiers::SUPER);
        assert_eq!(
            session.buffer().viewport().expect("viewport").first,
            expected
        );
    }
    press(&mut session, Key::Down, Modifiers::SUPER);
    assert_eq!(
        session.buffer().viewport().expect("viewport").first,
        prompts[5]
    );
    session.submit(Command::ScrollTo(ScrollTarget::Top), Origin::Api);
    session.pump(Instant::now());
    session.submit(
        Command::BindingAction {
            action: Action::JumpToPrompt(2),
            all: false,
        },
        Origin::User,
    );
    session.pump(Instant::now());
    assert_eq!(
        session.buffer().viewport().expect("viewport").first,
        prompts[2]
    );
    assert!(writes.lock().expect("writes").is_empty());
}

/// Removing history updates prompt marker fractions even without new output.
#[test]
fn clear_history_refreshes_prompt_sources() {
    let (mut session, _) = recorded();
    let before = session.viewport().scrollbar.revision();
    session.submit(
        Command::BindingAction {
            action: Action::ClearScrollback,
            all: false,
        },
        Origin::User,
    );
    session.pump(Instant::now());
    assert!(session.viewport().scrollbar.revision() > before);
    assert!(session.viewport().following());
    for marker in session.viewport().scrollbar.markers() {
        assert!(session.buffer().line(marker.line).is_some());
        assert!(marker.start >= 0.0 && marker.end <= 1.0);
    }
}

/// Stable command identities survive scroll/reflow and cannot select later reset output.
#[test]
fn zone_identity_and_line_alignment() {
    let (mut session, _, _) = fixture();
    let now = Instant::now();
    session.feed(
        b"\x1b]133;A\x07prompt\r\n\x1b]133;C\x07output\r\n\x1b]133;D;0\x07",
        now,
    );
    let id = session
        .buffer()
        .zones()
        .find(|m| m.kind == ZoneKind::Output)
        .expect("authored retained fixture")
        .id;
    let at = session
        .zone_output_point(id)
        .expect("authored retained fixture");
    session.submit(
        Command::Select(crate::selection::SelectionCommand::CommandOutput(Some(at))),
        Origin::Api,
    );
    session.pump(now);
    assert_eq!(session.selection_text(), "output\n");
    session.terminal.resize(40, 24);
    session.reconcile_selection();
    session.reconcile_viewport(false);
    assert!(session.zone_output_point(id).is_some());
    session.feed(
        b"\x1bc\x1b]133;A\x07p\r\n\x1b]133;C\x07new\r\n\x1b]133;D;0\x07",
        now,
    );
    assert!(session.zone_output_point(id).is_none());
    assert!(session.buffer().zones().all(|m| m.id > id));
    session.feed(&b"row\r\n".repeat(60), now);
    let line = session
        .terminal()
        .screen()
        .ring
        .retained(20)
        .expect("authored retained fixture")
        .id;
    session.submit(Command::ScrollTo(ScrollTarget::LineTop(line)), Origin::Api);
    session.pump(now);
    assert_eq!(session.viewport().top_index(), 20);
    session.submit(Command::ScrollTo(ScrollTarget::Line(line)), Origin::Api);
    session.pump(now);
    assert_eq!(session.viewport().top_index(), 8);
}
