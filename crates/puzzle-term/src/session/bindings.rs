//! Session ownership of shortcut state, timeouts and host action handoff.
//!
//! Matching is a public bus interceptor. The shell applies settings actions
//! after releasing the Session borrow, because the settings store is host-owned.
//!
//! The settings store is shared at the host boundary; the pending sequence
//! is not. Each session owns its buffered events and its own timeout epoch.
//! A settings reload flushes held keys before installing the new matcher.
//! Those keys resume after keybindings, preventing a replacement leader from
//! capturing an input event that predates the new table.
//! Host actions are queued until the platform releases its Session borrow.
//! This preserves the synchronous core without calling back into the native
//! settings store from a borrowed bus hook.
//! Timeout scheduling asks the existing Waker for one deadline only.
use super::Session;
use crate::{
    keybind::{Action, Keybindings},
    messages::Command,
};
use std::time::Duration;
use web_time::Instant;

/// Shortcut state is independent of GPU and operating-system handles.
impl Session {
    /// Replaces a validated table; buffered leaders are flushed first.
    pub fn set_keybindings(&mut self, mut bindings: Keybindings) {
        // A reload must not discard keystrokes already held by a leader.
        for (key, origin) in self.keybindings.flush_tagged() {
            self.bus.resume(Command::Key(key), origin, "keybindings");
        }
        bindings.now_ms = self.keybindings.now_ms;
        self.keybindings = bindings;
        self.help_table_changed();
        // Host OS changes alter the required link modifier and native-file policy.
        self.invalidate_link_hover();
    }
    /// Borrows effective entries and pending leader state for introspection.
    pub fn keybindings(&self) -> &Keybindings {
        &self.keybindings
    }
    /// Removes one host settings action without allocating a second queue.
    pub fn take_binding_action(&mut self) -> Option<(Action, bool)> {
        if self.binding_actions.is_empty() {
            None
        } else {
            Some(self.binding_actions.remove(0))
        }
    }
    /// Converts injected turn time and expires leaders before bus dispatch.
    pub(super) fn prepare_bindings(&mut self, now: Instant) {
        // Context is captured at the same drain boundary as modes and grid geometry.
        self.keybindings.query_input = self.model_overlays.query.open;
        self.keybindings.selection_available = self.selection.range().is_some();
        self.keybindings.output_available = self.selection.output_available;
        self.keybindings.search_available = self.search.state().count > 0;
        let epoch = *self.binding_epoch.get_or_insert(now);
        let millis = now
            .saturating_duration_since(epoch)
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        self.keybindings.now_ms = millis;
        if self.keybindings.timeout_remaining() == Some(0) {
            for (key, origin) in self.keybindings.flush_tagged() {
                self.bus.resume(Command::Key(key), origin, "keybindings");
            }
        }
    }
    /// A pending finite leader wakes once; the zero timeout schedules nothing.
    pub(super) fn schedule_bindings(&mut self, now: Instant) {
        self.sync_leader();
        if let Some(delay) = self.keybindings.timeout_remaining() {
            self.services
                .waker
                .wake_at(now + Duration::from_millis(delay));
        }
    }
}
