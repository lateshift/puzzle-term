//! Replays keyecho's actual two PTY groups through the D-33 settle rule.
//!
//! The child supplies mark and erase bytes across a real PTY; the script
//! selects its sleep gap. Every simulated display phase sees the first
//! eligible snapshot only after the erase for the measured 2/5/7-ms gaps.
//! The regular render-shell command separately checks the settled GPU ink.
//! This test does not infer timing from when the harness happened to read
//! from its channel: OS scheduling could merge both PTY writes into one read.
//! The child is asked to sleep for the chosen gap, and the replay injects
//! that gap into a manual monotonic clock for each possible display phase.
//! Matching bytes from the actual child prove the simulated first and
//! second updates correspond to the protocol the scenario just exercised.
//! A frame before the erase would read a caret in the show-command cell;
//! the assertion checks that cell at every eligible refresh tick.
use super::{Scenario, ShellRun};
use puzzle_term::{session::presentation::SettleWindow, vt::Terminal};
use std::time::Duration;
/// Two-second key holds reuse this real-PTY marker decoder.
mod repeat;
pub(in crate::harness) use repeat::expect_paced_repeat_no_marker;

/// Checks that a real child drew then erased a marker before presentation.
pub(in crate::harness) fn expect_paced_no_marker(
    state: &mut Scenario,
    line: &str,
) -> Result<(), String> {
    // Reject unsupported values before reading PTY output, so a typo
    // cannot quietly turn this into a different timing experiment.
    let gap = line
        .strip_prefix("expect-paced-no-marker ")
        .ok_or("expect-paced-no-marker needs gap")?
        .parse::<u64>()
        .map_err(|_| "invalid paced gap")?;
    if !matches!(gap, 2 | 5 | 7) {
        return Err("paced gap must be 2, 5 or 7 ms".into());
    }
    let Some(ShellRun::Active(probe)) = state.shell.as_ref() else {
        return Err("paced marker check needs active keyecho".into());
    };
    let bytes = probe
        .raw
        .get(probe.key_output_start..)
        .ok_or("key output offset exceeds captured PTY bytes")?;
    // The first synchronized group paints row 30, column 70. Its closing
    // mode reset is the application update boundary, not a PTY read edge.
    let mark = locate(bytes, b"\x1b[30;70H^").ok_or("keyecho did not draw a marker")?;
    let end = locate(&bytes[mark..], b"\x1b[?2026l")
        .ok_or("keyecho marker lacked a synchronized end")?
        + mark
        + 8;
    // The later group must explicitly clear that exact field. Checking
    // final pixels alone could pass if no marker had ever been written.
    let erase = locate(&bytes[end..], b"\x1b[30;70H          ")
        .ok_or("keyecho did not erase the marker")?;
    if erase == 0 {
        return Err("erase group did not follow the marker group".into());
    }
    check_phases(&bytes[..end], &bytes[end..], gap)
}

/// Finds a control sequence without decoding arbitrary user-visible bytes.
pub(super) fn locate(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Evaluates the captured groups at every half-ms offset of a 60-Hz clock.
fn check_phases(mark: &[u8], erase: &[u8], gap: u64) -> Result<(), String> {
    // A 60-Hz period covers the slowest common review display. Half-ms
    // offsets exercise refresh alignment rather than one lucky instant.
    let period = Duration::from_micros(16_667);
    for phase in 0..34 {
        // A fresh VT grid for each phase prevents a previous phase's erase
        // from making the next phase appear clean without processing bytes.
        let mut terminal = Terminal::new(100, 30, 0, 0);
        let mut settle = SettleWindow::new(Duration::from_millis(8));
        settle.presented(Duration::ZERO);
        // The prior presentation at zero makes the key's 20-ms mark an
        // idle burst, exactly when the settle rule must engage.
        let first = Duration::from_millis(20);
        terminal.advance(mark);
        // Confirm the first group is visibly wrong on its own. Otherwise
        // suppressing it would not demonstrate the reported flash.
        if terminal
            .screen()
            .row(29)
            .is_none_or(|row| row.cells[69].ch != u32::from('^'))
        {
            return Err("captured first group did not occupy the showcmd cell".into());
        }
        settle.request(first, period);
        let erase_at = first + Duration::from_millis(gap);
        let mut erased = false;
        // A host supplies refresh ticks; the shared rule suppresses all
        // earlier ticks, including one between these two PTY groups.
        let mut tick = Duration::from_micros(phase * 500);
        let mut presented = false;
        while tick < first + period * 3 {
            if tick >= erase_at && !erased {
                // An erase packet joins the same burst and never resets
                // the first-output time or postpones a valid deadline.
                terminal.advance(erase);
                settle.request(erase_at, period);
                erased = true;
            }
            if settle.may_snapshot(tick) {
                // Reading the actual VT cell here is the simulated
                // presented snapshot; the later GPU check covers ink.
                let marked = terminal
                    .screen()
                    .row(29)
                    .is_some_and(|row| row.cells[69].ch == u32::from('^'));
                if marked {
                    return Err(format!("phase {phase} presented showcmd marker"));
                }
                presented = true;
                break;
            }
            tick += period;
        }
        if !presented || !erased {
            return Err(format!("phase {phase} never reached a settled frame"));
        }
    }
    Ok(())
}
