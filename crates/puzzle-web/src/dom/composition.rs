//! Textarea composition ownership without premature or duplicate stdin (§12.2).
//!
//! Browsers disagree on whether the final input arrives before or after
//! compositionend. Provisional input updates preedit only; end commits once.
//! A matching trailing input is consumed, but an unrelated next insertion is
//! never discarded. A new key or composition clears the trailing-input guard.
use puzzle_term::messages::{Command, PreeditState};

/// One input-method transaction, separate from the negotiated keyboard mode.
#[derive(Default)]
pub(super) struct Composition {
    /// While composing, even an input with isComposing=false cannot send text.
    active: bool,
    /// Some browsers deliver the final non-composing input before end.
    final_before_end: bool,
    /// One possible trailing input already committed by compositionend.
    trailing: Option<String>,
}

/// Event order is normalized before the typed commands enter the Session.
impl Composition {
    /// A new physical key cannot be the trailing input of the previous IME.
    pub(super) fn new_key(&mut self) {
        self.trailing = None;
    }

    /// Starts provisional ownership; an abandoned previous transaction is cleared.
    pub(super) fn start(&mut self) -> Command {
        self.active = true;
        self.final_before_end = false;
        self.trailing = None;
        Command::Preedit(PreeditState::default())
    }

    /// Candidate changes are not application bytes, including an empty candidate.
    pub(super) fn update(&self, text: String, utf16_caret: Option<u32>) -> Command {
        Command::Preedit(PreeditState {
            // DOM offsets count UTF-16 units; the neutral wire counts UTF-8 bytes.
            cursor: utf16_caret.and_then(|caret| utf8_offset(&text, caret)),
            text,
        })
    }

    /// Clears preedit before sending exactly one final composition commit.
    pub(super) fn end(&mut self, text: String) -> Vec<Command> {
        self.active = false;
        self.trailing = (!self.final_before_end && !text.is_empty()).then(|| text.clone());
        let mut commands = vec![Command::Preedit(PreeditState::default())];
        // Cancellation removes the candidate but types no empty shell input.
        if !text.is_empty() {
            commands.push(Command::Text(text));
        }
        commands
    }

    /// Processes committed textarea input, including dead keys and mobile typing.
    pub(super) fn input(&mut self, text: String, composing: bool) -> Vec<Command> {
        if self.active || composing {
            // Safari can clear isComposing on its final input before end.
            self.final_before_end |= self.active && !composing;
            return vec![self.update(text, None)];
        }
        if self.trailing.take().as_ref() == Some(&text) || text.is_empty() {
            // Consume at most one echo of compositionend. Keeping the guard
            // indefinitely would eat a later, identical mobile insertion.
            return Vec::new();
        }
        vec![Command::Text(text)]
    }

    /// The textarea must keep its provisional value while an IME owns it.
    pub(super) fn active(&self) -> bool {
        self.active
    }
}

/// Rejects offsets inside a surrogate pair instead of slicing a Unicode scalar.
fn utf8_offset(text: &str, caret: u32) -> Option<usize> {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units == caret {
            return Some(byte);
        }
        units += ch.len_utf16() as u32;
    }
    (units == caret).then_some(text.len())
}

#[cfg(test)]
/// Both browser orders must type one commit, never a provisional character.
mod tests;
