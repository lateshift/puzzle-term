//! Effective shortcut sources are rebuilt only on input/settings/filter edges.
//!
//! The matcher owns leaders. Session owns the one query input mode. This bridge
//! samples them into immutable public provider data without granting a view keys.
//! Prefix matching is borrowed from Keybindings rather than reimplemented by UI.
//! No terminal rows are scanned and no command is executed while preparing help.
//!
//! Leader identity includes releases because ordered replay owns those original
//! events, but the public continuation iterator excludes releases from its grammar.
//! Comparing the short pending vector makes ordinary parser pumps constant-time
//! when no leader is active. Configuration replacement explicitly invalidates it.
//! Completing a swallowed leader still requests removal even without a child echo.
//!
//! Filter rows are formatted only while help is open and the committed filter or
//! effective table changes. Caret motion and provisional composition reuse them.
//! Source labels expose the winning layer rather than the overwritten lower entries.
//! Filtering includes group and layer names as well as chords and action arguments.
//! Immutable snapshot slots own their text; the UI cannot observe a later query edit.
//! Closing keeps prepared data available for a later open without a recurring wake.
use super::*;
use crate::keybind::Source;
use puzzle_overlay::models::help::BindingRow;
use std::fmt::Write;

impl Session {
    /// Registers help through public model/view contracts, with no keyboard callback.
    pub(super) fn install_help_overlays(&mut self) {
        self.overlays_mut().add(
            Box::new(puzzle_overlay::model::StaticModel::new(
                "leader-hint",
                EventKinds::ALL,
            )),
            Box::new(puzzle_overlay::builtins::help::LeaderView),
        );
        self.overlays_mut().add(
            Box::new(puzzle_overlay::builtins::help::Help::default()),
            Box::new(puzzle_overlay::builtins::help::HelpView),
        );
    }
    /// Changes table identity without forcing hidden help to allocate table labels.
    pub(in crate::session) fn help_table_changed(&mut self) {
        self.model_overlays.help_dirty = true;
        self.model_overlays.leader_dirty = true;
        // An open table or old leader may have visible ink that needs replacing.
        if self.model_overlays.query.open || !self.model_overlays.help.leader.is_empty() {
            self.overlay_changed(EventKinds::INPUT);
            self.needs_frame = true;
            self.services.waker.request_frame();
        }
    }
    /// Detects actual leader changes after input, including completion and expiry.
    pub(in crate::session) fn sync_leader(&mut self) {
        let state = &mut self.model_overlays;
        if state.pending_seen == self.keybindings.pending() && !state.leader_dirty {
            return;
        }
        // The pending vector retains capacity for later prefixes and release events.
        state.pending_seen.clear();
        state
            .pending_seen
            .extend_from_slice(self.keybindings.pending());
        state.leader_dirty = false;
        state.help.leader.clear();
        for (binding, count) in self.keybindings.continuations() {
            if !state.help.leader.is_empty() {
                state.help.leader.push_str(" · ");
            }
            // Canonical chords retain modifiers and physical/logical identities.
            let _ = write!(
                state.help.leader,
                "{}: {}",
                binding.steps[count].canonical(),
                binding.action.label()
            );
        }
        self.overlay_changed(EventKinds::INPUT);
        self.needs_frame = true;
        self.services.waker.request_frame();
    }
    /// Captures the current query mode without touching the incremental text search.
    pub(super) fn copy_help(&mut self, out: &mut Models) {
        let state = &mut self.model_overlays;
        // A caret-only edit leaves the committed filter equal and reuses its rows.
        let help = &mut state.help;
        let query = &state.query;
        let open = query.open && query.kind == query::Kind::Bindings;
        // A reopened panel may have retained labels from a different query mode.
        let rebuild = open && (!help.open || state.help_dirty || help.filter != query.text);
        help.open = open;
        if open {
            // Caret and composition changes reuse rows when the committed filter is equal.
            help.preedit.clone_from(&query.preedit);
            // This offset belongs to the local field, never the application's cursor.
            help.cursor = query.cursor;
        }
        if rebuild {
            help.filter.clone_from(&query.text);
            // Label preparation is an input/configuration operation, not frame work.
            help.rows.clear();
            let needle = help.filter.to_lowercase();
            // The matcher table is authoritative: overridden entries are already absent.
            for binding in self.keybindings.list() {
                let row = BindingRow {
                    trigger: binding.canonical(),
                    action: binding.action.label(),
                    group: binding.action.group(),
                    // Provenance is retained on the winning binding rather than inferred.
                    source: match binding.source {
                        Source::Defaults => "defaults",
                        Source::User => "user",
                        Source::Profile => "profile",
                        Source::Session => "session",
                    },
                };
                // Matching any displayed column makes below-fold entries discoverable.
                if [&row.trigger, &row.action, row.group, row.source]
                    .iter()
                    .any(|text| text.to_lowercase().contains(&needle))
                {
                    // Labels are owned by the source and later sealed into rotating slots.
                    help.rows.push(row);
                }
            }
            // Stable sorting preserves effective-table order inside each action group.
            help.rows.sort_by_key(|row| row.group);
        }
        // A hidden configuration edge is consumed only after this source sees it.
        state.help_dirty = false;
        // Nested strings and rows preserve the receiving publication's capacities.
        out.help.copy_from(help);
    }
}
