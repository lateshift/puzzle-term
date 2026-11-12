//! Unicode insertion/deletion for the Session-owned query input mode (§11.4).
//!
//! Byte indices always remain UTF-8 boundaries. Text/IME commits insert once;
//! provisional preedit never changes search results. The 4-KiB cap matches
//! ordinary bounded UI strings and rejects unbounded API paste allocation.
//!
//! This mode deliberately handles accepted keys before their application encoder.
//! Modifiers other than Shift do not insert characters, and key releases do not
//! mutate text. IME composing keys belong to the input method; only its eventual
//! Text commit replaces provisional ink with searched content. Enter carries a
//! signed result-navigation outcome rather than inserting a newline.
//!
//! Caret motions do not restart search work. Backspace removes the preceding
//! scalar and Delete removes the following scalar, keeping the stored byte index
//! valid at either end of an empty field. Home and End use exact string boundaries.
//! Paste follows the one-line field policy: printable Unicode is retained, while
//! every control including line breaks is omitted. The cap is checked per scalar,
//! so truncation never creates malformed UTF-8. Search results have their own M8 cap.
//! Preedit storage reuses its capacity but cannot change the committed query.
use crate::{
    messages::Command,
    vt::input::{Key, KeyPhase, Modifiers},
};
/// Distinguishes text search from effective-binding filtering.
#[derive(Default, PartialEq)]
pub(crate) enum Kind {
    /// Existing incremental buffer search.
    #[default]
    Search,
    /// Local shortcut filtering, without searching terminal rows.
    Bindings,
}

/// Editing state belongs to Session; the provider only sees a frozen copy.
#[derive(Default)]
pub(crate) struct Query {
    /// Open mode consumes every key until Escape or the close button.
    pub open: bool,
    /// Selects which source receives committed query changes.
    pub kind: Kind,
    /// Committed literal query, without a copy of any search results.
    pub text: String,
    /// Insertion position as a UTF-8 byte boundary.
    pub cursor: usize,
    /// Uncommitted input-method candidate.
    pub preedit: String,
    /// Source edge invalidates its retained provider exactly once.
    pub dirty: bool,
}
/// Input outcomes distinguish local presentation from model mutation/navigation.
pub(crate) enum Edit {
    /// Command does not belong to this mode, e.g. terminal output/focus.
    Pass,
    /// Consumed input only moved the caret or changed provisional composition.
    Consumed,
    /// Committed query changed and must replace the existing M8 query.
    Changed,
    /// Close/clear the mode and its results.
    Close,
    /// Enter or Shift+Enter navigates already-discovered matches.
    Step(i32),
}
impl Query {
    /// Edits accepted input without encoding application bytes.
    pub(crate) fn edit(&mut self, command: &Command) -> Edit {
        if !self.open {
            return Edit::Pass;
        }
        match command {
            Command::Text(text) | Command::Paste(text) => {
                // Text includes IME commits and paste; provisional composition is discarded.
                self.insert(text);
                Edit::Changed
            }
            Command::Preedit(state) => {
                // Preserve the candidate allocation across composition updates (P1).
                self.preedit.clone_from(&state.text);
                self.dirty = true;
                Edit::Consumed
            }
            Command::Key(key) if key.composing || key.phase == KeyPhase::Release => Edit::Consumed,
            Command::Key(key) => self.key(key),
            _ => Edit::Pass,
        }
    }
    /// Physical editing keys remain local even when the same key has an app binding.
    fn key(&mut self, key: &crate::vt::input::KeyEvent) -> Edit {
        // Every editing motion repaints the caret without restarting the search scan.
        self.dirty = true;
        match key.key {
            Key::Escape => Edit::Close,
            Key::Enter | Key::KeypadEnter => {
                Edit::Step(if key.modifiers.0 & Modifiers::SHIFT != 0 {
                    -1
                } else {
                    1
                })
            }
            Key::Backspace => {
                // Drain whole scalars, never part of a multibyte committed character.
                let before = self.previous();
                self.text.drain(before..self.cursor);
                self.cursor = before;
                Edit::Changed
            }
            Key::Delete => {
                // Forward delete leaves the caret at its original boundary.
                let after = self.next();
                self.text.drain(self.cursor..after);
                Edit::Changed
            }
            Key::Left => {
                self.cursor = self.previous();
                Edit::Consumed
            }
            Key::Right => {
                self.cursor = self.next();
                Edit::Consumed
            }
            Key::Home => {
                self.cursor = 0;
                Edit::Consumed
            }
            Key::End => {
                self.cursor = self.text.len();
                Edit::Consumed
            }
            Key::Char(ch) if key.modifiers.0 & !Modifiers::SHIFT == 0 => {
                // Physical plain keys are used only when the host has not delegated text.
                self.insert(ch.encode_utf8(&mut [0; 4]));
                Edit::Changed
            }
            _ => Edit::Consumed,
        }
    }
    /// Inserts only bounded printable Unicode, ignoring control injection/newlines.
    fn insert(&mut self, text: &str) {
        self.preedit.clear();
        // Stop at a scalar boundary and remove all line/control input from this one-line UI.
        for ch in text.chars().filter(|ch| !ch.is_control()) {
            if self.text.len() + ch.len_utf8() > 4096 {
                break;
            }
            self.text.insert(self.cursor, ch);
            self.cursor += ch.len_utf8();
        }
        self.dirty = true;
    }
    /// Finds the preceding scalar boundary, with start-of-field remaining stable.
    fn previous(&self) -> usize {
        // Reverse char_indices yields the start of the previous scalar in byte units.
        self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index)
    }
    /// Finds the following boundary without ever indexing into a continuation byte.
    fn next(&self) -> usize {
        // Adding the next scalar length keeps End/Delete stable at the empty suffix.
        self.text[self.cursor..]
            .chars()
            .next()
            .map_or(self.cursor, |ch| self.cursor + ch.len_utf8())
    }
}
