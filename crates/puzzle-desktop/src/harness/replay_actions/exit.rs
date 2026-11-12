//! Platform child-exit records in a diagnostic replay (§4.7, §13.4).
//!
//! Recordings are external input. A forged ChildExited must never bypass the
//! public bus's Internal visibility rule, so only this trusted replay path
//! translates a validated record into a platform-origin command.
use super::Replay;
use crate::recording;
use puzzle_term::{
    Origin,
    messages::{Command, ExitInfo},
};
use serde_json::Value as Json;
use std::time::Instant;

impl Replay {
    /// Replays a trusted platform exit or compares one input adapter result.
    pub(super) fn command_entry(
        &mut self,
        number: usize,
        data: &Json,
        now: Instant,
    ) -> Result<(), String> {
        // The stable internal type string is the only record that may be
        // submitted with Platform origin; RawInput commands must still match
        // the adapter. Coalesced pointer commands have their own wire witness.
        if data.get("type").and_then(Json::as_str) == Some("internal/childExited") {
            // A missing payload is never equivalent to an empty ExitInfo:
            // its runtime controls the abnormal-exit hold rule.
            let info = recorded_exit(data.get("payload").ok_or("exit payload missing")?)?;
            let command = Command::ChildExited(info);
            // Re-encode to reject extra fields and malformed numeric values
            // before an external recording can affect Session lifetime.
            if recording::command_json(Some(&command))? != *data {
                return Err(format!("entry {number}: child exit payload changed"));
            }
            self.session.submit(command, Origin::Platform);
            // Pumping at the recorded instant preserves the command's order
            // relative to the preceding PTY reads and following frame entry.
            self.session.pump(now);
            return Ok(());
        }
        // Never bypass an outstanding adapter comparison, even for a pointer
        // type: missing RawInput is valid only at the coalesced pointer seam.
        if self.command.is_none() && self.pointer_entry(number, data, now)? {
            return Ok(());
        }
        // Input-generated commands are checked against the current adapter;
        // a changed encoding is reported at its original recording entry.
        let actual = self
            .command
            .take()
            .ok_or("command without preceding input")?;
        // Compare the whole typed projection, not just PTY bytes: a handler
        // may turn two different commands into the same wire sequence.
        if &actual != data {
            return Err(format!(
                "entry {number}: adapter command changed: recorded {data}, new {actual}"
            ));
        }
        Ok(())
    }
}

/// Validates the internal exit record without exposing it as public wire input.
fn recorded_exit(value: &Json) -> Result<ExitInfo, String> {
    // Code null represents a signaled child; zero remains a real exit code.
    let code = match value.get("code") {
        Some(Json::Null) => None,
        Some(Json::Number(number)) => {
            // JSON numbers can be fractional, so conversion must check the
            // integer range before using a narrowing cast.
            let raw = number.as_f64().ok_or("exit code is not numeric")?;
            if !raw.is_finite()
                || raw.fract() != 0.0
                || raw < f64::from(i32::MIN)
                || raw > f64::from(i32::MAX)
            {
                // An unchecked float-to-int cast would saturate and make a
                // corrupt recording appear to contain a plausible status.
                return Err("exit code outside i32".into());
            }
            Some(raw as i32)
        }
        _ => return Err("exit code missing".into()),
    };
    // Absence is distinct from JSON null: all recorders write a signal field,
    // and its omission would make a truncated entry look valid.
    let signal = match value.get("signal") {
        Some(Json::Null) => None,
        Some(Json::String(name)) => Some(name.clone()),
        _ => return Err("exit signal missing".into()),
    };
    // Runtime is mandatory because an early nonzero exit forces Hold even
    // when the session configuration otherwise requests Close.
    let runtime = value
        .get("runtimeMs")
        .and_then(Json::as_f64)
        .ok_or("exit runtime missing")?;
    // A JavaScript number is exact only through 2^53 - 1; rejecting larger
    // values prevents a replay from changing its exit policy at that edge.
    if !runtime.is_finite()
        || runtime.fract() != 0.0
        || runtime < 0.0
        || runtime > ((1_u64 << 53) - 1) as f64
    {
        return Err("exit runtime loses JSON precision".into());
    }
    let runtime_ms = runtime as u64;
    // Only the fields emitted by the recorder reach this typed command;
    // command_json above rejects any extra object members after conversion.
    Ok(ExitInfo {
        code,
        signal,
        runtime_ms,
    })
}
