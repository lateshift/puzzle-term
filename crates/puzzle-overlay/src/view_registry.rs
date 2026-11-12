//! Permanent renderer ownership of overlay views and rebuildable draw caches.
//!
//! Snapshot versions govern rebuilds. Views never travel back to the Session;
//! accepting even skipped publications delivers registrations before their
//! slots are recycled. Source data and model state remain immutable here.
//!
//! Each rotating slot may describe an earlier publication than another slot.
//! Build matching therefore compares the view's last consumed model version
//! with the version in the slot actually selected for drawing. It does not
//! trust a slot's dirty flag or assume the previous publication was built.
//! The full identity set also makes removals robust to skipped snapshots.
//! Prepared commands retain relative anchors: their placement may change on
//! a grid-only frame without invoking an otherwise clean view callback.
use crate::{
    anchor::Resolved,
    draw::DrawList,
    model::SharedState,
    registry::ProviderId,
    view::{OverlayView, ViewCtx},
};
use puzzle_vt::terminal::Terminal;

/// Latest immutable payload for one model in one rotating snapshot slot.
pub(crate) struct State {
    /// Provider identity, never a reused vector index.
    pub(crate) id: ProviderId,
    /// Model update counter, independent of publication frequency.
    pub(crate) version: u64,
    /// Shared immutable payload; unchanged states keep their allocation.
    pub(crate) state: SharedState,
}
/// Overlay values accompany a caller-owned frame snapshot.
#[derive(Default)]
pub struct OverlaySnapshot {
    /// Latest versions for all still registered models.
    pub(crate) states: Vec<State>,
    /// One-time view transfers; accept these even if this frame is skipped.
    pub(crate) views: Vec<(ProviderId, Box<dyn OverlayView>)>,
}
/// One retained drawing cache, entirely reconstructible from its immutable state.
struct Entry {
    /// Stable registration order for painter ties.
    id: ProviderId,
    /// Drawing callbacks cannot mutate the Session's model.
    view: Box<dyn OverlayView>,
    /// Last rendered version; None forces the first build.
    version: Option<u64>,
    /// Painter operations retain capacity between dirty builds.
    list: DrawList,
}
/// Renderer-side provider owner, separate from ModelRegistry.
#[derive(Default)]
pub struct ViewRegistry {
    /// Permanent callbacks and rebuildable output.
    entries: Vec<Entry>,
    /// Changes invalidate compiled overlay instances, not model state.
    revision: u64,
}
/// Snapshot delivery and drawing require no Session access or synchronization.
impl ViewRegistry {
    /// Receives registrations before a snapshot is either drawn or skipped.
    pub fn accept(&mut self, snapshot: &mut OverlaySnapshot) {
        // Retire removed models even when this publication has no overlay buffer at all.
        // A skipped frame still carries the full identity set; stale effects must not draw.
        let before = self.entries.len();
        self.entries
            .retain(|entry| snapshot.states.iter().any(|state| state.id == entry.id));
        if self.entries.len() != before {
            self.revision = self.revision.wrapping_add(1);
        }
        for (id, view) in snapshot.views.drain(..) {
            self.entries.push(Entry {
                id,
                view,
                version: None,
                list: DrawList::default(),
            });
        }
    }
    /// Rebuilds changed views and resolves anchors against the frozen visible terminal.
    pub fn build(
        &mut self,
        snapshot: &OverlaySnapshot,
        ctx: &ViewCtx<'_>,
        terminal: &Terminal,
        pointer: Option<[f32; 2]>,
    ) -> bool {
        let before = self.entries.len();
        // A latest snapshot contains the full registration set, so skipped removals are safe.
        self.entries
            .retain(|entry| snapshot.states.iter().any(|state| state.id == entry.id));
        let mut changed = before != self.entries.len();
        for entry in &mut self.entries {
            let Some(state) = snapshot.states.iter().find(|state| state.id == entry.id) else {
                continue;
            };
            // Only a changed model value invalidates this view's retained commands.
            if entry.version != Some(state.version) {
                // A view may reclaim owned label buffers as rebuildable scratch.
                entry.view.recycle(&mut entry.list);
                entry.list.draws.clear();
                entry.view.build(state.state.as_ref(), ctx, &mut entry.list);
                entry.version = Some(state.version);
                changed = true;
            }
            for draw in &mut entry.list.draws {
                // Attachments move with snapshot metadata without re-running idle views.
                let resolved = draw.anchor.sample(terminal, pointer);
                changed |= draw.resolved != resolved;
                draw.resolved = resolved;
            }
            // Eviction is permanent; viewport hiding retains commands for later return.
            entry
                .list
                .draws
                .retain(|draw| !(draw.auto_remove && draw.resolved == Resolved::Gone));
        }
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        changed
    }
    /// Empty views avoid geometry preparation for sessions without overlays.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Borrows painter output without producing a fresh command vector per frame.
    pub fn lists(&self) -> impl Iterator<Item = (ProviderId, &DrawList)> {
        self.entries.iter().map(|entry| (entry.id, &entry.list))
    }
    /// Names the compiled command revision independently of model publication.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}
