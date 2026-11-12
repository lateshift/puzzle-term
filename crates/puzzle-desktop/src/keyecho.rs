//! Native full-screen witness for PTY key encoding and presentation timing.
//!
//! The harness starts this binary behind `stty raw -echo` in a real PTY.
//! Reading its replies proves the emulator actually accepted negotiation;
//! the fixture never substitutes a predicted kitty mode for that reply.
//! Screen updates use only ANSI/DEC controls already exercised by nvim.
//! A temporary show-command marker followed by an erase makes the two
//! update boundaries visible to drawn readback without private editor data.
//! Query replies and user keys can share an OS read. The decoder emits them
//! in byte order so the kitty push occurs before later Ctrl-key reports.
//! An ignored query reply never becomes RAW user input on the screen.
//! This tool is a desktop binary mode, so its stdout is the PTY protocol,
//! not a library log; diagnostics elsewhere still use `tracing` (R10).

use std::io::{Read, Write};
/// Bounded byte decoder for legacy and kitty key reports.
mod decode;
/// Alternate-screen paints and numbered-scroll commands.
mod screen;
use decode::{Decoder, Event};
use screen::Screen;

/// Runs the full-screen test child until the PTY closes it.
pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let (flags, gap, repeat_ms) = parse_flags(args)?;
    run_io(flags, gap, repeat_ms).map_err(|error| error.to_string())
}

/// Keeps OS read/write failures distinct from argument validation.
fn run_io(flags: u8, gap: u64, repeat_ms: Option<u64>) -> std::io::Result<()> {
    // The child remains the PTY's raw protocol endpoint; terminal echo
    // would add a second copy of every Ctrl byte and spoil the witness.
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    let mut screen = Screen::with_timing(flags, gap, repeat_ms);
    // Initial queries precede the first input so a mode-1 key is encoded
    // only after the terminal has acknowledged its keyboard capability.
    screen.start(&mut output)?;
    let mut decoder = Decoder::default();
    let mut bytes = [0_u8; 1024];
    loop {
        // A PTY may split CSI at any byte. Decoder retains an incomplete
        // suffix until the next read, while the OS read stays bounded.
        let count = input.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        decoder.feed(&bytes[..count]);
        while let Some(event) = decoder.next() {
            match event {
                Event::SyncSupported => screen.set_sync(true),
                Event::KittyReply => screen.enable_kitty(&mut output)?,
                Event::Ignore => {}
                Event::Key(key) => screen.key(&mut output, key)?,
                Event::Raw(raw) => screen.raw(&mut output, &raw)?,
            }
        }
        output.flush()?;
    }
    // The harness normally kills the child. A clean EOF still restores
    // screen and keyboard modes for an interactive manual invocation.
    screen.finish(&mut output)?;
    output.flush()
}

/// Accepts only the protocol configurations and erase gaps under review.
fn parse_flags(args: &[String]) -> Result<(u8, u64, Option<u64>), String> {
    // A fixed argument grammar makes the PTY witness reproducible across
    // machines and rejects accidental changes to the measured cadence.
    let [switch, value, rest @ ..] = args else {
        return Err(
            "usage: puzzle-desktop keyecho --kitty 0|1|3|31 [--gap 2|5|7] [--repeat-ms 33]".into(),
        );
    };
    if switch != "--kitty" {
        return Err("keyecho needs --kitty FLAGS".into());
    }
    let flags: u8 = value.parse().map_err(|_| "invalid kitty flags")?;
    if !matches!(flags, 0 | 1 | 3 | 31) {
        return Err("keyecho flags must be 0, 1, 3 or 31".into());
    }
    let (gap, repeat_ms) = match rest {
        // Old K4 scenarios keep the original two-ms timing. The explicit
        // K5b values span the later 3.2–6.4 ms editor measurement.
        [] => (2, None),
        [name, value] if name == "--gap" => (value.parse().map_err(|_| "invalid gap")?, None),
        [gap_name, gap_value, repeat_name, repeat_value]
            if gap_name == "--gap" && repeat_name == "--repeat-ms" =>
        {
            // Repeats are opt-in so the 108 single-key matrix cases retain
            // their existing one-command, one-erase behavior.
            (
                gap_value.parse().map_err(|_| "invalid gap")?,
                Some(
                    repeat_value
                        .parse()
                        .map_err(|_| "invalid repeat interval")?,
                ),
            )
        }
        _ => return Err("keyecho needs --gap MS then --repeat-ms 33".into()),
    };
    if !matches!(gap, 2 | 5 | 7) {
        return Err("keyecho gap must be 2, 5 or 7 ms".into());
    }
    if repeat_ms.is_some_and(|interval| interval != 33) {
        // The real-child scenario uses 33 ms; the neutral sweep separately
        // covers the user's observed 33.4-ms inter-key interval.
        return Err("keyecho repeat interval must be 33 ms".into());
    }
    Ok((flags, gap, repeat_ms))
}

#[cfg(test)]
/// Protocol decoding and mode validation live with this binary witness.
mod tests;
