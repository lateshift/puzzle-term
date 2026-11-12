//! Shortcut specimens use the actual matcher and the built-in Session query mode.
//!
//! Effective rows come from real configuration layers. The leader is a buffered
//! key, never fabricated draw data. Both reference scenes keep a visible cursor
//! so cursor anchoring has the same semantics as the interactive terminal.
//!
//! A default leader offers two actions while an unrelated user binding offers
//! search. Profile zoom and session help rows expose all four winning layers.
//! Source rows are fed through the same parser and precedence merger as config.
//! Any malformed fixture entry fails before a screenshot can supply evidence.
//! The fixture never injects a help caption, filter result or rectangle directly.
//! A normal key starts the leader or invokes showKeybindings through bus policy.
//! The short prompt leaves space below the caret for the real cursor-relative pill.
//! Blink is disabled solely to make successive reference captures deterministic.
//! DPR and physical image bounds remain the harness's responsibility.
use crate::{
    Session,
    keybind::{Keybindings, Platform, Source},
    messages::Command,
    vt::input::{Key, KeyEvent},
};
use puzzle_bus::Origin;
use web_time::Instant;

/// Installs a representative layered table and opens the requested public UI.
pub(super) fn install(session: &mut Session, leader: bool) -> Result<(), String> {
    let now = Instant::now();
    session.install_model_overlays();
    session.set_cursor_settings(
        crate::session::cursor_settings::CursorSettings {
            blink: crate::session::cursor_settings::CursorBlink::Off,
            ..Default::default()
        },
        now,
    );
    let mut table = Keybindings::default();
    // Each row shows the winning source instead of an overwritten default.
    for (source, entries) in [
        (
            Source::Defaults,
            vec!["a>c=copy".into(), "a>d=sendText:hello".into()],
        ),
        (Source::User, vec!["x=search".into()]),
        (Source::Profile, vec!["z=zoom:2".into()]),
        (Source::Session, vec!["h=showKeybindings".into()]),
    ] {
        let errors = table.apply(&entries, source, Platform::Mac, false);
        if !errors.is_empty() {
            return Err(format!("shortcut specimen: {errors:?}"));
        }
    }
    session.set_keybindings(table);
    session.feed(b"\x1b[3;3H$ ", now);
    session.submit(
        Command::Key(KeyEvent::press(Key::Char(if leader { 'a' } else { 'h' }))),
        Origin::User,
    );
    session.pump(now);
    Ok(())
}
