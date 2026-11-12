//! Scenario entry point and JSON Lines traversal for action replay.
//!
//! Replayed frame boundaries come from the recorded sequence, while
//! private frame diagnostics stay under the ignored target directory.
//! The reader keeps original JSON Lines indices, since a changed adapter
//! write must identify the event the user actually captured. Presentation
//! entries are replayed in file order even when millisecond timestamps tie:
//! the recorder's ordering is more precise than its clock resolution.
//! Drawn mode adds offscreen GPU evidence to that same chronology, so a
//! diagnosis can compare a live-style retained frame with a cold rebuild.

use super::super::Scenario;
use super::Replay;
use crate::recording;
use serde_json::Value as Json;
use std::path::Path;

/// Replays a `.pzrec` from a scenario and writes each presented grid.
pub(crate) fn run(state: &mut Scenario, line: &str) -> Result<(), String> {
    // The scenario path points to a private target file unless its owner
    // separately reviewed and committed a neutralized fixture.
    let mut parts = line.split_whitespace();
    let _ = parts.next();
    // Mode changes only the experiment; it never changes the recording's
    // interpretation or silently drops events from the deterministic path.
    let path = parts.next().ok_or("replay-actions needs a file")?;
    let mode = match parts.next() {
        None => "ordinary",
        Some("--input-only") => "input-only",
        Some("--drawn-every-frame") => "drawn",
        Some(other) => return Err(format!("unknown replay-actions option: {other}")),
    };
    if mode == "input-only" {
        // The live-child experiment does not compare VT frames because a
        // new process may legitimately produce a different screen.
        return input_to_shell(state, Path::new(path));
    }
    replay_file_mode(Path::new(path), &state.output, mode == "drawn")
}

/// Replays an action file without needing a GPU or live child process.
#[cfg(test)]
pub(super) fn replay_file(path: &Path, output: &Path) -> Result<(), String> {
    replay_file_mode(path, output, false)
}

/// Optionally draws each recorded snapshot with fresh and retained caches.
fn replay_file_mode(path: &Path, output: &Path, with_drawn: bool) -> Result<(), String> {
    let source = recording::read(path)?;
    let mut replay = None;
    // Keep the replay state absent until a validated header fixes the grid.
    // That prevents a corrupt file from starting with a mid-stream parse.
    for (index, line) in source.lines().enumerate() {
        // Each JSON line is independently parsed. A broken final line
        // reports its index instead of partially replaying it silently.
        let entry: Json =
            serde_json::from_str(line).map_err(|error| format!("entry {}: {error}", index + 1))?;
        let kind = recording::field_str(&entry, "k")?;
        let data = entry.get("data").ok_or("recording entry lacks data")?;
        if kind == "header" {
            // One header fixes the grid for all following parse and frame
            // entries; a second could otherwise reset VT mid-recording.
            if replay.is_some() {
                return Err("duplicate recording header".into());
            }
            replay = Some(Replay::new(data, with_drawn)?);
        } else {
            replay.as_mut().ok_or("recording lacks header")?.entry(
                index + 1,
                kind,
                data,
                output,
                recording::field_u64(&entry, "t")?,
            )?;
        }
    }
    let replay = replay.ok_or("recording has no header")?;
    if let Some(probe) = replay.drawn.as_ref() {
        // Only counts leave the private drawn files: screen text from a
        // user recording must remain in the ignored output directory.
        let (frames, divergent) = probe.totals();
        std::fs::write(
            output.join("replay-drawn-summary.txt"),
            format!("frames={frames} cold_differences={divergent}\n"),
        )
        .map_err(|error| error.to_string())?;
    }
    replay.finish()
}

/// Sends only recorded host callbacks to the currently active shell.
fn input_to_shell(state: &mut Scenario, path: &Path) -> Result<(), String> {
    // Re-run only the host callbacks. PTY output and recorded writes belong
    // to the original child and must never be injected into this new one.
    let source = recording::read(path)?;
    for (index, line) in source.lines().enumerate() {
        let entry: Json =
            serde_json::from_str(line).map_err(|error| format!("entry {}: {error}", index + 1))?;
        if recording::field_str(&entry, "k")? == "input" {
            // RawInput is validated before the same adapter used by the
            // native window maps it into a typed Session command.
            let data = entry.get("data").ok_or("input lacks data")?;
            super::super::shell::raw_input(state, recording::raw_from_json(data)?)?;
        }
    }
    Ok(())
}
