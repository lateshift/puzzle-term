//! Quiet-period waiting for live PTY scenario commands (§13.1).
//!
//! A full-screen application may emit several reads for one redraw. A
//! fixed sleep would either inspect a partial frame or waste time on every
//! assertion. This command resets its quiet timer after each actual read
//! and has an independent maximum so a noisy child cannot hang acceptance.
//!
//! The quiet timer measures time since the most recent PTY read, not since
//! the command was sent. An application may respond in several bursts:
//! cursor movement, text paint and mode exit can arrive in distinct reads.
//! Resetting after each read prevents a test from observing the middle of
//! that sequence merely because the first read came quickly.
//!
//! The `max` deadline is separate from quiet time. A continuously animated
//! program might never become silent, and the harness must report that as
//! a failed assertion within a bounded time. The two durations are parsed
//! from explicit units so `50` cannot accidentally mean seconds instead
//! of milliseconds. Thirty seconds is the largest allowed test wait.
//!
//! A disconnected PTY reader is considered quiet only after the requested
//! quiet interval has elapsed. This also lets a short-lived command such
//! as `printf first` finish without keeping its shell child alive solely
//! to satisfy a test. All received bytes still enter Session before the
//! next grid dump, preserving their order and parser state.

use super::{Scenario, ShellRun};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

/// Waits for PTY silence without assuming an application's redraw duration.
pub(crate) fn wait_idle(state: &mut Scenario, line: &str) -> Result<(), String> {
    // The script grammar spells both intervals, including `max`, so a
    // transposed value cannot silently turn into an unbounded sleep.
    let mut words = line.split_whitespace();
    let _command = words.next();
    let quiet = duration(words.next().ok_or("wait-idle needs quiet time")?)?;
    if words.next() != Some("max") {
        return Err("wait-idle needs max".into());
    }
    let limit = duration(words.next().ok_or("wait-idle needs limit")?)?;
    if words.next().is_some() || quiet > limit {
        // Quiet cannot exceed the maximum; such a script could never pass.
        return Err("invalid wait-idle range".into());
    }
    let Some(ShellRun::Active(probe)) = state.shell.as_mut() else {
        // An absent child is a scenario error, not a successful idle state.
        return Err("wait-idle needs active shell".into());
    };
    let deadline = Instant::now() + limit;
    let mut last_output = Instant::now();
    // Recompute remaining quiet time on every loop because an output chunk
    // resets its origin, while the absolute maximum never moves.
    while Instant::now() < deadline {
        let remaining_quiet = quiet.saturating_sub(last_output.elapsed());
        if remaining_quiet.is_zero() {
            return Ok(());
        }
        let wait = remaining_quiet.min(deadline.saturating_duration_since(Instant::now()));
        // A blocking receive avoids polling the channel while still waking
        // immediately when the PTY produces another piece of a redraw.
        match probe.output.recv_timeout(wait) {
            Ok(bytes) => {
                // The same ingestion path retains raw evidence and feeds
                // the actual Session parser before quiet time restarts.
                probe.ingest(&bytes);
                last_output = Instant::now();
            }
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {
                // A timeout may be the absolute deadline rather than the
                // quiet deadline, so check elapsed quiet time explicitly.
                if last_output.elapsed() >= quiet {
                    return Ok(());
                }
            }
        }
    }
    Err("wait-idle exceeded maximum time".into())
}

/// Parses a bounded manual quiet/timeout value such as 50ms or 2s.
fn duration(text: &str) -> Result<Duration, String> {
    // Milliseconds and seconds are sufficient for deterministic scenario
    // waits; fractional parsing would make the grammar locale-sensitive.
    let (number, multiplier) = if let Some(value) = text.strip_suffix("ms") {
        (value, 1)
    } else if let Some(value) = text.strip_suffix('s') {
        (value, 1000)
    } else {
        return Err("duration needs ms or s".into());
    };
    let count: u64 = number.parse().map_err(|_| "invalid duration")?;
    let millis = count.checked_mul(multiplier).ok_or("duration overflow")?;
    // Reject absurd waits before they stall the entire acceptance command.
    if millis > 30_000 {
        return Err("duration exceeds 30s".into());
    }
    Ok(Duration::from_millis(millis))
}
