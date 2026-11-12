//! Portable shortcut parsing and deterministic sequence matching (§14.9).
//!
//! The session owns this state; the bus borrows it as an interceptor during
//! a pump. No platform dependency or shared mutable pointer is needed for
//! introspection. Physical codes and layout characters remain independent.
//!
//! A binding table is selected by the host's OS, not the compilation target.
//! This matters for wasm: a Mac user needs Command even though the binary
//! was built on another platform. Resolution happens before layer merging.
//! Only effective entries remain in the table; Source records which layer
//! last supplied each entry. Pending keys belong to the individual session,
//! so a leader in one terminal cannot consume input from another terminal.
//! The injected millisecond counter also lets tests drive timeouts exactly.
mod action;
mod action_parse;
/// Alternate action spellings accepted by the portable parser.
pub mod aliases;
/// Platform tables are data, tested on every host.
pub mod defaults;
mod matcher;
mod parser;
/// Borrowed prefix introspection and catalogue presentation.
mod presentation;
mod trigger;
use crate::vt::input::KeyEvent;
pub use action::{ACTIONS, Action, ActionInfo};
pub use parser::{Binding, Entry, Prefixes};
pub use trigger::Trigger;

/// User OS, passed in by the shell rather than inferred in the core.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Platform {
    /// Command is the primary application shortcut modifier.
    Mac,
    /// Windows uses Control and performable interrupt/copy shortcuts.
    Windows,
    /// Linux uses Control with Shift to avoid readline conflicts.
    #[default]
    Linux,
}

/// Resolves host modifier conventions before parsing portable strings.
impl Platform {
    /// Resolves primary before canonicalizing or matching a trigger.
    pub fn primary(self) -> u8 {
        if self == Self::Mac { 8 } else { 4 }
    }
}

/// Provenance attached to each effective binding for CLI and future JS UI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    /// Schema's platform-specific initial table.
    Defaults,
    /// Persistent config.json or page storage data.
    User,
    /// Active named profile overrides.
    Profile,
    /// Transient CLI or terminal options.
    Session,
}

/// Effective table and buffered sequence, owned by one terminal session.
#[derive(Clone, Debug, Default)]
pub struct Keybindings {
    /// Later layers replace this entry by canonical trigger identity.
    entries: Vec<Binding>,
    /// Leader keys awaiting either a completion or an ordered flush.
    pending: Vec<KeyEvent>,
    /// Each held press/release retains its trusted source until ordered replay.
    pending_origins: Vec<puzzle_bus::Origin>,
    /// Injected time of the last accepted leader step.
    started_ms: u64,
    /// Zero deliberately means wait forever; no idle timer is needed.
    pub timeout_ms: u64,
    /// Input time is supplied by Session, never read from a platform clock.
    pub(crate) now_ms: u64,
    /// Contextual copy availability captured at the current input drain.
    pub(crate) selection_available: bool,
    /// Completed shell output supports performable copyCommandOutput.
    pub(crate) output_available: bool,
    /// Contextual next/previous availability follows surviving query results.
    pub(crate) search_available: bool,
    /// Web pages supply their own UI; desktop search bar arrives in M9.
    pub(crate) search_ui: bool,
    /// Host type chooses native Session query mode versus page-owned web UI.
    pub(crate) web: bool,
    /// Host convention determines primary-modifier link gestures.
    pub(crate) platform: Platform,
    /// An open Session query owns physical keys before shortcuts/PTY encoding.
    pub(crate) query_input: bool,
    /// One browser press may delegate unmatched text to its input event.
    pub(crate) native_key: Option<KeyEvent>,
    /// The current native press passed policy without becoming encoder bytes.
    pub(crate) native_passed: bool,
}

/// Maintains the effective table separately from a pending leader.
impl Keybindings {
    /// Borrows effective bindings with their original settings layers.
    pub fn list(&self) -> &[Binding] {
        &self.entries
    }
    /// Enumerates stable actions, including those awaiting later milestones.
    pub fn actions(&self) -> &'static [ActionInfo] {
        ACTIONS
    }
    /// Exposes buffered keys so future leader overlays can show continuations.
    pub fn pending(&self) -> &[KeyEvent] {
        &self.pending
    }
    /// Dry-runs a single key without advancing the leader state.
    pub fn test(&self, key: &KeyEvent) -> Option<&Binding> {
        if key.composing {
            return None;
        }
        self.candidate(std::slice::from_ref(key))
    }
    /// Applies a source in order and reports every rejected/deferred entry.
    pub fn apply(
        &mut self,
        lines: &[String],
        source: Source,
        platform: Platform,
        web: bool,
    ) -> Vec<String> {
        self.search_ui = true;
        self.web = web;
        self.platform = platform;
        let mut issues = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            match Entry::parse(line, platform, source) {
                Ok(Entry::Clear) => self.entries.clear(),
                Ok(Entry::Binding(binding)) => {
                    // Prefixes do not change identity. A performable copy
                    // therefore replaces an earlier ordinary copy binding.
                    self.entries
                        .retain(|old| old.canonical() != binding.canonical());
                    if binding.action != Action::Unbind {
                        if binding.action.info().available != "now" {
                            issues.push(format!(
                                "[{index}] available from {}: {}",
                                binding.action.info().available,
                                binding.action.info().name
                            ));
                        }
                        if web && binding.reserved(platform) {
                            issues.push(format!(
                                "[{index}] browser-reserved shortcut: {}",
                                binding.canonical()
                            ));
                        }
                        self.entries.push(binding);
                    }
                }
                Err(error) => issues.push(format!("[{index}] {error}")),
            }
        }
        issues
    }
}

#[cfg(test)]
/// Public parser, matcher and layering behavior under hostile configuration.
mod tests;
