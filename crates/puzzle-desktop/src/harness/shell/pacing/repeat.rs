//! Held Ctrl+D replay through the shared settle decision (K5c).
//!
//! Keyecho emits each mark and erase through a real PTY. The harness then
//! replays those captured groups at the requested repeat cadence for every
//! display phase, checking the VT cell at each eligible presentation tick.
use super::{Scenario, SettleWindow, ShellRun, Terminal, locate};
use std::time::Duration;

/// One captured synchronized mark and its later erase group.
struct Pair<'a> {
    /// Complete first group, including its closing mode-2026 sequence.
    mark: &'a [u8],
    /// Complete second group, including scroll and marker erasure.
    erase: &'a [u8],
}

/// Checks a two-second held key at the selected real-child erase gap.
pub(in crate::harness) fn expect_paced_repeat_no_marker(
    state: &mut Scenario,
    line: &str,
) -> Result<(), String> {
    // This command checks only measured editor gaps; arbitrary values could
    // claim coverage outside the short-lived marker case under review.
    let gap = line
        .strip_prefix("expect-paced-repeat-no-marker ")
        .ok_or("held marker check needs a gap")?
        .parse::<u64>()
        .map_err(|_| "invalid held marker gap")?;
    if !matches!(gap, 2 | 5) {
        return Err("held marker gap must be 2 or 5 ms".into());
    }
    let Some(ShellRun::Active(probe)) = state.shell.as_ref() else {
        return Err("held marker check needs active keyecho".into());
    };
    // Input fences exclude startup negotiation and prior keys; otherwise a
    // stale marker could satisfy the count without testing this hold.
    let bytes = probe
        .raw
        .get(probe.key_output_start..)
        .ok_or("held key output offset exceeds PTY bytes")?;
    let pairs = split_pairs(bytes)?;
    // Half-ms phase steps straddle both sides of a two-ms marker window
    // without relying on the host compositor's current timing.
    for phase in 0..34 {
        check_phase(&pairs, gap, phase)?;
    }
    Ok(())
}

/// Extracts all 60 update pairs from actual mode-2026 PTY boundaries.
fn split_pairs(bytes: &[u8]) -> Result<Vec<Pair<'_>>, String> {
    // A sync close is an application-update boundary even when one OS read
    // merges several writes from this fast child.
    const END: &[u8] = b"\x1b[?2026l";
    let mut cursor = 0;
    let mut pairs = Vec::with_capacity(60);
    // The configured child makes one press plus 59 repeats. Counting groups
    // also detects an incomplete capture before timing is simulated.
    for _ in 0..60 {
        // Search only after the previous erase; a caret in older status
        // text cannot stand in for this repeat's new show-command update.
        let mark_at = cursor
            + locate(&bytes[cursor..], b"\x1b[30;70H^D").ok_or("held key lacks a showcmd mark")?;
        // Include the first group's mode reset, not only its visible caret.
        // Otherwise the VT would appear to remain inside synchronized mode.
        let mark_end =
            mark_at + locate(&bytes[mark_at..], END).ok_or("held mark lacks sync end")? + END.len();
        // The second group must clear the exact field painted above. A
        // scroll alone is insufficient evidence that the flash was removed.
        let erase_at = mark_end
            + locate(&bytes[mark_end..], b"\x1b[30;70H          ")
                .ok_or("held key lacks an erase")?;
        // The final mode reset makes the erase eligible for a later frame.
        let erase_end = erase_at
            + locate(&bytes[erase_at..], END).ok_or("held erase lacks sync end")?
            + END.len();
        // Slices refer to the PTY capture rather than allocating sixty
        // copies of two terminal update streams.
        pairs.push(Pair {
            mark: &bytes[cursor..mark_end],
            erase: &bytes[mark_end..erase_end],
        });
        cursor = erase_end;
    }
    // An extra mark would mean the simulated two-second count was wrong;
    // a missing one was rejected inside the loop above.
    if locate(&bytes[cursor..], b"\x1b[30;70H^D").is_some() {
        return Err("held key produced more than 60 marks".into());
    }
    Ok(pairs)
}

/// Replays captured groups against one half-ms offset of a 60-Hz display.
fn check_phase(pairs: &[Pair<'_>], gap: u64, phase: u64) -> Result<(), String> {
    // At 60 Hz the refresh falls in every possible half-ms position as
    // phase varies; the same output times are reused for each position.
    let period_us = 16_667;
    let period = Duration::from_micros(period_us);
    let mut terminal = Terminal::new(100, 30, 0, 0);
    // A fresh VT and settle state prevent the previous phase's final erase
    // from masking a marker in this phase.
    let mut settle = SettleWindow::new(Duration::from_millis(8));
    let mut tick_us = phase * 500;
    let mut presented = 0;
    for (index, pair) in pairs.iter().enumerate() {
        // Keyecho uses whole 33-ms intervals. The neutral unit sweep also
        // checks the user's observed 33.4-ms cadence independently.
        let mark_us = 20_000 + index as u64 * 33_000;
        let erase_us = mark_us + gap * 1_000;
        while tick_us < mark_us {
            // A settled earlier repeat may present here; that clears only
            // the pending burst, while last output still tracks silence.
            if settle.may_snapshot(Duration::from_micros(tick_us)) {
                settle.presented(Duration::from_micros(tick_us));
                presented += 1;
            }
            tick_us += period_us;
        }
        // The first captured group must actually draw a caret. Without this
        // witness an empty stream would pass every presentation assertion.
        terminal.advance(pair.mark);
        if !marked(&terminal) {
            return Err(format!("phase {phase} repeat {index} did not paint ^D"));
        }
        settle.request(Duration::from_micros(mark_us), period);
        // An eligible refresh between mark and erase would display ^D even
        // though the final grid becomes correct a few milliseconds later.
        while tick_us < erase_us {
            if settle.may_snapshot(Duration::from_micros(tick_us)) {
                return Err(format!("phase {phase} repeat {index} presented ^D"));
            }
            tick_us += period_us;
        }
        terminal.advance(pair.erase);
        // Reject a captured stream that never actually erases the marker;
        // scheduling alone cannot make such a terminal frame correct.
        if marked(&terminal) {
            return Err(format!("phase {phase} repeat {index} failed to erase ^D"));
        }
        // The erase joins the mark's burst and must not reset its deadline.
        settle.request(Duration::from_micros(erase_us), period);
    }
    // Each 33-ms interval should allow one settled frame. Requiring many
    // presentations prevents a rule that simply never draws from passing.
    if presented < 40 {
        return Err(format!("phase {phase} presented only {presented} frames"));
    }
    Ok(())
}

/// Reads the same show-command cell used by the drawn-grid assertion.
fn marked(terminal: &Terminal) -> bool {
    // CSI 30;70H is one-based, so its caret occupies zero-based row 29,
    // column 69 in the terminal grid.
    terminal
        .screen()
        .row(29)
        .is_some_and(|row| row.cells[69].ch == u32::from('^'))
}
