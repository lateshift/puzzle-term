//! Session-owned context path, never tied to screen contents or RIS epochs.
//! UAPI.15 preserves older safety contexts on overflow and across terminal reset.
//! Change notifications are coarse invalidations: Session publishes the final
//! path once per parser drain, copying it only if a bus observer requests it.
use super::{Terminal, TerminalEvent};
use crate::context::{Context, MAX_DEPTH};

impl Terminal {
    /// Borrow the active root-to-leaf path; the last entry is currently active.
    pub fn contexts(&self) -> &[Context] {
        &self.contexts
    }

    /// Replace a matching context and end its descendants, or push a new child.
    /// Existing IDs can still be updated at the depth limit: a return to an
    /// ancestor releases its descendants before the host learns the new path.
    pub(crate) fn start_context(&mut self, context: Context) {
        if let Some(index) = self.contexts.iter().position(|c| c.id == context.id) {
            // Reinitialization clears all old metadata, even when fields are omitted.
            self.contexts.truncate(index + 1);
            self.contexts[index] = context;
        } else if self.contexts.len() < MAX_DEPTH {
            self.contexts.push(context);
        } else {
            // UAPI explicitly says keep earlier contexts, not evict the oldest.
            tracing::debug!(
                event = "vt.context_full",
                limit = MAX_DEPTH,
                "new context ignored; earlier contexts preserved"
            );
            return;
        }
        // Queue an invalidation, not a copy of every intermediate context path.
        self.events.push(TerminalEvent::ContextsChanged);
    }

    /// End a known context and its descendants; unknown IDs change nothing.
    pub(crate) fn end_context(&mut self, id: &str) {
        if let Some(index) = self.contexts.iter().position(|c| c.id == id) {
            self.contexts.truncate(index);
            self.events.push(TerminalEvent::ContextsChanged);
        }
    }

    /// Session teardown, including a held final screen, ends every active context.
    /// Returns whether a final empty-path fact is needed; RIS never calls this.
    pub fn clear_contexts(&mut self) -> bool {
        let changed = !self.contexts.is_empty();
        // Drop all metadata but reuse the bounded path allocation for a later owner.
        self.contexts.clear();
        changed
    }
}
