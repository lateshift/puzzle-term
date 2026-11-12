//! Restores high-frequency input recorded after coalescing (§5.7, P12).
//!
//! RawInput is the keyboard/IME/focus seam; Pointer, Wheel and Mouse are already
//! typed when recorded. Decode only those public kinds, and round-trip the whole
//! record to reject extra fields or forged origins. Keys retain adapter checks.
use super::Replay;
use crate::{recording, settings::json::from_json};
use puzzle_term::{
    Origin,
    bus::wire::{ExtensionRegistry, FromWire},
    messages::Command,
};
use serde_json::Value as Json;
use std::time::Instant;

impl Replay {
    /// Returns false for non-pointer kinds; malformed pointer records fail closed.
    pub(super) fn pointer_entry(
        &mut self,
        number: usize,
        data: &Json,
        now: Instant,
    ) -> Result<bool, String> {
        // An absent type and every unrelated command remain subject to the
        // caller's RawInput requirement; this is a closed list of native seams.
        let Some(kind @ ("input/pointer" | "input/wheel" | "input/mouse")) =
            data.get("type").and_then(Json::as_str)
        else {
            return Ok(false);
        };
        // Share the bounded JSON converter with settings rather than introducing
        // a second untrusted-value decoder at the native platform boundary.
        let payload = from_json(data.get("payload").ok_or("pointer payload missing")?, 0)?;
        let command = Command::from_value(kind, &payload, &ExtensionRegistry::default())
            .map_err(|error| format!("entry {number}: invalid pointer command: {error:?}"))?;
        // Shape validation alone may accept future fields. The current recorder's
        // exact projection is the evidence being replayed, including its numbers.
        if recording::command_json(Some(&command))? != *data {
            return Err(format!("entry {number}: pointer payload changed"));
        }
        // Origin is supplied by this trusted seam, never by an external payload.
        // Pump now so selection and mouse reports precede the following snapshot.
        self.session.submit(command, Origin::User);
        self.session.pump(now);
        Ok(true)
    }
}
