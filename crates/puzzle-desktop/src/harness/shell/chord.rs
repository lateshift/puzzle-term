//! Scenario chord parser for encoder-only `key` commands (§13.1).
//!
//! `key` bypasses the platform RawInput sequence and submits one typed
//! VtKeyEvent. That keeps it useful for isolating the M5 encoder from a
//! platform callback sequence recorded later by `input` (§13.4).
//!
//! Modifier tokens precede exactly one logical key token. The parser
//! produces a typed KeyEvent, leaving legacy and kitty byte choices to
//! Session's live terminal mode. That mode may have changed after the
//! preceding PTY read, so precomputing bytes in the script would be wrong.
//!
//! The input vocabulary deliberately covers common ASCII controls and
//! cursor movement. Unknown names fail visibly rather than being sent as
//! text: an accidental `ctrl+shift+d` typo must not reach an editor as an
//! unrelated printable string. The full configurable keybinding grammar
//! belongs to M7, while this command remains an encoder-only probe.

use super::{Scenario, ShellRun};
use puzzle_term::Origin;
use puzzle_term::vt::input::{Key, KeyEvent, Modifiers};
use std::time::Instant;

/// Parses modifiers before the final logical key name.
pub(super) fn parse(raw: &str) -> Result<KeyEvent, String> {
    // `+` separates modifiers from the final key; taking the last token
    // leaves their order irrelevant but preserves duplicate-bit idempotence.
    let mut parts: Vec<&str> = raw.split('+').collect();
    let name = parts.pop().ok_or("empty key chord")?;
    let mut bits = 0;
    // These positions are the neutral kitty/xterm bits, never winit's
    // backend-specific modifier representation.
    for part in parts {
        bits |= match part {
            "ctrl" => Modifiers::CTRL,
            "alt" => Modifiers::ALT,
            "shift" => Modifiers::SHIFT,
            "super" => Modifiers::SUPER,
            _ => return Err(format!("unknown key modifier: {part}")),
        };
    }
    let key = match name {
        // Named terminal controls retain their identity until Session
        // samples DECCKM, kitty flags and other active input modes.
        "enter" => Key::Enter,
        "tab" => Key::Tab,
        "escape" => Key::Escape,
        "backspace" => Key::Backspace,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        _ => {
            // One Unicode scalar is representable as Key::Char. A longer
            // composition belongs in a Text command, not this key grammar.
            let mut chars = name.chars();
            let first = chars.next().ok_or("empty key name")?;
            if chars.next().is_some() {
                return Err(format!("unknown key name: {name}"));
            }
            Key::Char(first)
        }
    };
    let mut event = KeyEvent::press(key);
    // The scenario requests a press only. Release phases are exercised
    // through the recorded RawInput sequence used by `input`.
    event.modifiers = Modifiers(bits);
    Ok(event)
}

/// Sends a general chord through the typed VT encoder without RawInput.
///
/// Write indices fence the key's synchronous pump. Later terminal-query
/// replies are protocol output too, but they are not additional key sends.
/// The flags snapshot makes a legacy byte explainable when a test races
/// negotiation rather than silently accepting it.
pub(crate) fn key(state: &mut Scenario, line: &str) -> Result<(), String> {
    let raw = line.strip_prefix("key ").ok_or("key needs a chord")?;
    let event = parse(raw)?;
    let Some(ShellRun::Active(probe)) = state.shell.as_mut() else {
        return Ok(());
    };
    // Fence the synchronous key write separately from later PTY replies.
    probe.key_start = probe
        .writes
        .lock()
        .map_err(|error| error.to_string())?
        .len();
    // This is the mode that `Session::pump` will sample, not a mode inferred
    // from raw bytes that may still sit in the parser's pending queue.
    let terminal = probe.session.terminal();
    probe.key_flags = terminal.kitty_keys.flags(terminal.active);
    // Submit a typed KeyEvent; direct byte injection would bypass the
    // exact encoder path whose nvim behavior the user reported.
    probe
        .session
        .submit(puzzle_term::messages::Command::Key(event), Origin::User);
    probe.session.pump(Instant::now());
    // Closing the fence now excludes asynchronous replies triggered by
    // nvim's next redraw, leaving precisely the input-related writes.
    probe.key_end = probe
        .writes
        .lock()
        .map_err(|error| error.to_string())?
        .len();
    Ok(())
}
