//! Immutable shortcut-help sources sampled by Session (design §11.6).
//!
//! Effective table labels are prepared only on input, filter or settings edges.
//! Views borrow these strings and cannot mutate the matching table or query mode.
//! Snapshot copies reuse existing nested allocations rather than cloning per frame.

/// One effective entry, with its original layer and discoverable action group.
#[derive(Default, PartialEq)]
pub struct BindingRow {
    /// Canonical chord sequence, identical to matcher introspection.
    pub trigger: String,
    /// Human-readable action and its validated argument.
    pub action: String,
    /// Semantic catalogue group, independent of settings layer.
    pub group: &'static str,
    /// Highest contributing configuration layer.
    pub source: &'static str,
}

/// Nested row labels retain capacity when rotating publication slots are reused.
impl Clone for BindingRow {
    /// A genuinely new row owns its labels independently of the source owner.
    fn clone(&self) -> Self {
        Self {
            trigger: self.trigger.clone(),
            action: self.action.clone(),
            group: self.group,
            source: self.source,
        }
    }
    /// Vec::clone_from delegates here instead of replacing warmed Strings (P1).
    fn clone_from(&mut self, source: &Self) {
        self.trigger.clone_from(&source.trigger);
        self.action.clone_from(&source.action);
        self.group = source.group;
        self.source = source.source;
    }
}

/// Session's input mode and leader presentation, sealed with the frame's sources.
#[derive(Default)]
pub struct Help {
    /// Empty when no leader is pending; no timer belongs to its drawing view.
    pub leader: String,
    /// The cheat sheet owns input only while this field is true.
    pub open: bool,
    /// Committed filter text; matching never searches terminal contents.
    pub filter: String,
    /// Provisional composition, which never becomes application input.
    pub preedit: String,
    /// UTF-8 insertion boundary in the committed filter.
    pub cursor: usize,
    /// Filtered effective entries, grouped by their action catalogue group.
    pub rows: Vec<BindingRow>,
}
impl Help {
    /// Retains warmed buffers across snapshot slot rotations (P1).
    pub fn copy_from(&mut self, source: &Self) {
        // clone_from on each nested value retains its text capacity where possible.
        self.leader.clone_from(&source.leader);
        self.open = source.open;
        self.filter.clone_from(&source.filter);
        self.preedit.clone_from(&source.preedit);
        self.cursor = source.cursor;
        self.rows.clone_from(&source.rows);
    }
}
