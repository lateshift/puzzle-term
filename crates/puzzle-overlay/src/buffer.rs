//! Sealed visible BufferView for render-side overlay views (D-50).
//!
//! The overlay frame reads the same row identities as the grid snapshot. It
//! must not borrow the live parser during shaping, nor copy all scrollback.
//! This bounded read-only window preserves BufferView's cell/link/zone API;
//! offscreen rows are deliberately unavailable, just like hidden anchors.
//! Copies happen only for dirty providers. Unchanged row versions and cluster
//! String capacities are retained, and no font or GPU resource enters VT.
//!
//! This is the user-approved M9 provider-context contract: callbacks see
//! exactly the late snapshot's visible rows on both desktop and web. Full
//! buffer access belongs to Session's non-frame queries. In particular,
//! logical-line URL detection and OSC 8 grouping are Session work, because
//! a wrapped link may continue beyond either viewport edge.
//!
//! The detached terminal is a read-only adapter, never a second parser of
//! application output. Its ring contains only the visible window; no
//! scrollback is copied. Row identity and version decide whether cells need
//! copying. Screen epoch prevents equal IDs on alternate screens from
//! exposing stale primary-screen text.
//!
//! Sparse cell extras own cluster strings, which must keep their capacity
//! on a warmed copy. Metadata snapshots likewise reuse VT registry/link/
//! zone allocations. A dirty callback can query those facts without locks;
//! an idle provider does not trigger even this bounded copy.
use puzzle_vt::{row::Row, terminal::Terminal};

/// Reused visible-row window and small terminal facts for provider callbacks.
pub struct BufferSnapshot {
    /// A detached terminal supplies the existing borrowed BufferView API.
    terminal: Terminal,
    /// Screen incarnation invalidates equal row IDs after a screen switch.
    epoch: u64,
    /// Lazy dense block mapping, bounded by this visible window.
    blocks: Option<Box<puzzle_vt::ring::BlockSnapshotScratch>>,
}
impl Default for BufferSnapshot {
    /// A minimal parser exists only to reuse VT's established read-only facade.
    fn default() -> Self {
        Self {
            terminal: Terminal::new(2, 1, 0, 0),
            epoch: u64::MAX,
            blocks: None,
        }
    }
}
/// Copies a bounded window into the same read-only facade used by Session.
impl BufferSnapshot {
    /// Copies only visible changed rows and metadata before the parser resumes.
    pub fn capture(&mut self, source: &Terminal, top: usize, count: usize) {
        self.capture_rows(source, count, &|index| top + index);
    }

    /// Seals noncontiguous source identities for a reversible user projection.
    /// The caller supplies retained indices in document order, never copies cells.
    pub fn capture_rows(
        &mut self,
        source: &Terminal,
        count: usize,
        source_row: &impl Fn(usize) -> usize,
    ) {
        let columns = source.size().0;
        if self.terminal.size() != (columns, count.max(1)) {
            // This detached read-only grid never reflows; source IDs already did.
            self.terminal.reflow_enabled = false;
            self.terminal.resize(columns, count.max(1));
        }
        self.terminal.active = source.active;
        for index in 0..count {
            if let (Some(row), Some(target)) = (
                source.screen().ring.retained(source_row(index)),
                self.terminal.screen_mut().ring.visible_mut(index),
            ) {
                // Content and screen epochs decide copies, not viewport row indices.
                if target.id != row.id
                    || target.version != row.version
                    || self.epoch != source.screen_epoch
                {
                    copy_row(target, row);
                }
            }
        }
        if source
            .screen()
            .ring
            .multicells
            .as_ref()
            .is_some_and(|store| !store.is_empty())
        {
            // Seal payloads before any view can resolve a publication-local cell key.
            self.terminal.screen_mut().ring.snapshot_blocks_rows(
                &source.screen().ring,
                source_row,
                self.blocks.get_or_insert_with(Default::default),
            );
        } else {
            self.terminal.screen_mut().ring.multicells = None;
            self.blocks = None;
        }
        self.epoch = source.screen_epoch;
        self.terminal.modes.clone_from(&source.modes);
        self.terminal.tracked.snapshot_from(&source.tracked);
        self.terminal.links.snapshot_from(&source.links);
        self.terminal.zones.snapshot_from(&source.zones);
        self.terminal.title.clone_from(&source.title);
        self.terminal.cwd.clone_from(&source.cwd);
        self.terminal.colors = source.colors;
        self.terminal.cursor_style = source.cursor_style;
        self.terminal.cursor = source.cursor;
        // A scrolled-out cursor must not resolve onto an unrelated copied row.
        if let Some(line) = source.cursor_line()
            && let Some(index) = self.terminal.screen().ring.index_of(line)
        {
            self.terminal.cursor.row = index;
        } else {
            self.terminal.modes.set(true, 25, false);
        }
    }
    /// Borrows the frozen VT-shaped window without owning another row copy.
    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }
}
/// Preserves row and sparse cluster storage during dirty-provider snapshots.
fn copy_row(target: &mut Row, source: &Row) {
    target.id = source.id;
    target.version = source.version;
    target.flags = source.flags;
    target.blocks.clone_from(&source.blocks);
    target.cells.clone_from(&source.cells);
    target.extras.truncate(source.extras.len());
    for (index, extra) in source.extras.iter().enumerate() {
        if let Some(owned) = target.extras.get_mut(index) {
            // A derived clone would replace the owned cluster on every edit.
            owned.cluster.clone_from(&extra.cluster);
            owned.column = extra.column;
            owned.underline_color = extra.underline_color;
            owned.link_id = extra.link_id;
        } else {
            target.extras.push(extra.clone());
        }
    }
}
