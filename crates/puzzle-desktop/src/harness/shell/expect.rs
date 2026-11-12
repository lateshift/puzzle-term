//! Compares actual PTY write boundaries with the active M5 key table.
//!
//! The simulator reads those bytes independently, while this assertion
//! catches a platform adapter that sends a duplicate or stale-mode key.
//! A write fence surrounds the complete RawInput sequence in the harness.
//! That fence includes a kitty release when requested, but excludes later
//! VT device-query replies triggered by the child's next screen update.
//! The expected rows come from the already table-tested M5 pure encoder;
//! this assertion tests the desktop mapping and Session mode sampling.
//! It cannot by itself certify the M5 encoder table, which has its own
//! xterm and kitty source-cited tests in puzzle-vt.
//! Legacy flags omit releases entirely. Kitty's progressive enhancement
//! may emit both press and release as separate transport writes; comparing
//! a concatenated byte stream would miss the accidental extra call that
//! originally motivated the Ctrl+D diagnosis.

use super::{Scenario, ShellRun};
use crate::keyboard::{
    raw::{RawInput, RawLogical},
    sequences,
};

/// Asserts real PTY writes against the M5 encoder's active-mode table.
pub(crate) fn expect_pty_key(state: &mut Scenario, line: &str) -> Result<(), String> {
    use puzzle_term::vt::input::{InputModes, Key, KeyEvent, KeyPhase, Modifiers, encode_key};
    let chord = line
        .strip_prefix("expect-pty-key ")
        .ok_or("expect-pty-key needs chord")?;
    let (events, _) = sequences::for_chord(chord)?;
    // The same sequence supplies the press's layout-aware character and
    // modifier bits, so expected bytes match the user's selected chord.
    let Some(ShellRun::Active(probe)) = state.shell.as_ref() else {
        return Err("expect-pty-key needs active shell".into());
    };
    let Some(RawInput::Key(press)) = events.get(1) else {
        return Err("missing press".into());
    };
    let Some(RawInput::Modifiers(bits)) = events.first() else {
        return Err("missing modifiers".into());
    };
    let RawLogical::Character(text) = &press.logical else {
        return Err("missing letter".into());
    };
    let RawLogical::Character(base) = &press.base else {
        return Err("missing base letter".into());
    };
    let letter = base.chars().next().ok_or("empty base letter")?;
    let modes = InputModes::from_terminal(probe.session.terminal());
    // Session samples this active screen after the child negotiated kitty.
    // Predicting from command-line flags could miss a 1049 stack switch.
    let mut event = KeyEvent::press(Key::Char(letter));
    event.modifiers = Modifiers(*bits);
    // M7.2 adds kitty's physical-US alternate independently of layout text.
    // The recorded A-Z fixture names Code(KeyX); the published position table
    // supplies its scalar, while only Shift supplies a shifted alternate.
    event.base = press
        .physical
        .strip_prefix("Code(")
        .and_then(|code| code.strip_suffix(')'))
        .and_then(puzzle_term::vt::input::physical::us_scalar);
    event.shifted = (*bits & Modifiers::SHIFT != 0)
        .then(|| text.chars().next())
        .flatten();
    let mut expected = vec![encode_key(&event, modes)];
    event.phase = KeyPhase::Release;
    expected.push(encode_key(&event, modes));
    expected.retain(|bytes| !bytes.is_empty());
    // A legacy keyup has no wire form. Empty vectors therefore represent
    // no transport call, not a zero-byte call that should be counted.
    let writes = probe.writes.lock().map_err(|error| error.to_string())?;
    let actual = writes
        .get(probe.key_start..probe.key_end)
        .ok_or("key fence invalid")?;
    if actual != expected {
        return Err(format!(
            "{chord}: PTY writes differ: actual={actual:?} expected={expected:?}"
        ));
    }
    Ok(())
}
