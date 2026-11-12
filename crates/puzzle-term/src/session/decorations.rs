//! Session's declarative annotation owner and mutation API (design §11.3).
//!
//! Immutable definitions travel to frame callbacks by Arc. Rewrite witnesses
//! reuse VT's selection stability engine: it owns separate tracked endpoints,
//! checks covered row versions (including identical rewrites), and rebases after
//! a content-preserving reflow. This does not create a user selection.
//! At most 256 declarations and 4096 witnessed rows per declaration are retained.
//!
//! Caller markers and lifetime witnesses are distinct owners. Removing an
//! annotation must not invalidate a marker shared with another declaration.
//! The witness registers its own two endpoints and releases exactly those
//! endpoints when the definition is removed or replaced.
//!
//! A failed update validates before replacing any entry. Its parsed SVG and
//! source interval are accepted together, so malformed geometry leaves the
//! previous declaration, region and rewrite witness unchanged.
//! ID allocation occurs only after validation; disposed IDs are never reused.
//!
//! The record limit bounds retained text/SVG strings as well as provider
//! draw-list size. A separate row-span limit bounds lifetime version vectors.
//! This matters for a range across history: its definition is small, but
//! checking a million rows would otherwise allocate a million witnesses.
//!
//! Reconciliation happens at source changes and before capture, including
//! when source text is offscreen. Frame callbacks cannot decide that hidden
//! text was rewritten from their visible-only BufferView, so Session owns
//! that decision and invalidates the provider using a coarse event flag.
//!
//! The VT witness rejects a rewrite even if identical characters replace
//! the old cells. It also remembers screen incarnation and logical content
//! across reflow. A valid reflow rebases the physical row-version list;
//! scrolling alone preserves source versions and the annotation lifetime.
//!
//! Snapshot records hold Arc references to immutable definitions. A parser
//! hook can edit the session-owned model while a render-side view finishes;
//! that frame continues to see its captured definition until the next turn.
//! Removing the last annotation unregisters its lazy built-in provider.
use super::Session;
use puzzle_overlay::{
    anchor::Resolved,
    decoration::{Decoration, DecorationId, Sample},
    provider::EventKinds,
    registry::ProviderId,
    shapes::ShapeRef,
};
use puzzle_vt::{
    Terminal,
    selection::{SelectPoint, Selection, SelectionKind},
};
use std::sync::Arc;
/// Validation and one-time SVG parsing stay at the mutation boundary.
mod validate;
/// One source definition and its private content-lifetime witness.
struct Entry {
    /// Stable identity doubles as the built-in provider's local region ID.
    id: DecorationId,
    /// Definitions are copied only on actual API edits, never during frames.
    declaration: Arc<Decoration>,
    /// Parsed SVG shares the same lifetime as its immutable declaration.
    shape: Option<ShapeRef>,
    /// Separate tracked points prevent annotation removal from releasing caller markers.
    witness: Option<Selection>,
}
/// Definition ownership remains with Session while frame callbacks run elsewhere.
#[derive(Default)]
pub(super) struct Store {
    /// Bounded cold API storage; reconciliation retains survivor allocations.
    entries: Vec<Entry>,
    /// Removed IDs never alias another annotation.
    next: u64,
    /// One lazy public-API provider renders all kinds and tooltips.
    provider: Option<ProviderId>,
}
/// Copies only small records and shared immutable definitions at snapshot time.
impl Store {
    /// Captures source positions; providers never query offscreen rows themselves.
    /// Immutable Arc owners retain their text/path allocations through an external frame.
    /// Clearing the destination drops only prior references and preserves its Vec capacity.
    /// The Session owner resolves markers into immutable frame data before publication.
    pub(super) fn capture(
        &self,
        term: &Terminal,
        pointer: Option<[f32; 2]>,
        out: &mut Vec<Sample>,
    ) {
        out.clear();
        for entry in &self.entries {
            // Retain source SVG and text without re-parsing or cloning their command buffers.
            out.push(Sample {
                id: entry.id,
                declaration: entry.declaration.clone(),
                resolved: entry.declaration.anchor.sample(term, pointer),
                shape: entry.shape.clone(),
            });
        }
    }
}
/// Borrowed controller ensures every mutation invalidates the singular renderer.
pub struct DecorationsMut<'a>(&'a mut Session);
/// Adds, edits and disposes typed annotations without exposing source witnesses.
impl DecorationsMut<'_> {
    /// Adds a bounded definition, rejecting invalid sources before registering any state.
    /// The ID is returned only after SVG and source lifetime validation succeed.
    /// A failed addition neither consumes an ID nor installs the lazy renderer.
    /// Successful addition requests one frame through Session's normal Waker.
    pub fn add(&mut self, declaration: Decoration) -> Result<DecorationId, String> {
        let session = &mut self.0;
        if session.decorations.entries.len() >= 256 {
            return Err("at most 256 decorations are retained".into());
        }
        let entry = make_entry(
            &mut session.terminal,
            declaration,
            DecorationId(session.decorations.next),
        )?;
        // Identity allocation follows validation, so a failed SVG cannot leave partial state.
        session.decorations.next = session.decorations.next.saturating_add(1);
        let id = entry.id;
        session.decorations.entries.push(entry);
        session.decoration_changed();
        Ok(id)
    }
    /// Replaces a definition atomically; failed validation leaves old content intact.
    /// Source witness replacement releases only privately registered endpoints.
    /// Captured frames may still hold the previous immutable definition until completion.
    /// Subsequent capture sees the replacement through a coarse dirty event.
    pub fn update(&mut self, id: DecorationId, declaration: Decoration) -> Result<(), String> {
        let session = &mut self.0;
        let Some(index) = session
            .decorations
            .entries
            .iter()
            .position(|entry| entry.id == id)
        else {
            return Err("unknown decoration identity".into());
        };
        let replacement = make_entry(&mut session.terminal, declaration, id)?;
        // Release only the private witness, never the source markers supplied by the caller.
        if let Some(witness) = session.decorations.entries[index].witness.take() {
            witness.clear(&mut session.terminal);
        }
        session.decorations.entries[index] = replacement;
        session.decoration_changed();
        Ok(())
    }
    /// Disposes one definition; repeated removal is harmless and returns false.
    pub fn remove(&mut self, id: DecorationId) -> bool {
        let session = &mut self.0;
        let Some(index) = session
            .decorations
            .entries
            .iter()
            .position(|entry| entry.id == id)
        else {
            return false;
        };
        let entry = session.decorations.entries.remove(index);
        if let Some(witness) = entry.witness {
            witness.clear(&mut session.terminal);
        }
        session.decoration_changed();
        true
    }
    /// Reads one current declaration without copying its text or SVG data.
    pub fn get(&self, id: DecorationId) -> Option<&Decoration> {
        self.0
            .decorations
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.declaration.as_ref())
    }
}
/// Creates private rewrite witnesses only for text-attached auto-remove content.
/// Fixed panels have no source-row versions, while Gone tracked handles are rejected.
/// VT validates both endpoints before registering its private lifetime handles.
/// The physical row-span cap is checked before allocating the version witness vector.
fn make_entry(
    term: &mut Terminal,
    declaration: Decoration,
    id: DecorationId,
) -> Result<Entry, String> {
    let shape = validate::definition(&declaration)?;
    let resolved = declaration.anchor.sample(term, None);
    if resolved == Resolved::Gone {
        return Err("decoration anchor is already gone".into());
    }
    let witness = if declaration.auto_remove
        && let Resolved::Cells(range) = resolved
    {
        // Bound version storage before VT captures the covered physical rows.
        if range.end.line.0.abs_diff(range.start.line.0) >= 4096 {
            return Err("decoration lifetime interval exceeds 4096 rows".into());
        }
        let point = |p: puzzle_overlay::geometry::Cell| SelectPoint {
            line: p.line,
            column: p.column,
        };
        Some(
            Selection::new(
                term,
                SelectionKind::Simple,
                point(range.start),
                point(range.end),
            )
            .ok_or("invalid decoration source interval")?,
        )
    } else {
        None
    };
    Ok(Entry {
        id,
        declaration: Arc::new(declaration),
        shape,
        witness,
    })
}
/// Scheduling and source reconciliation run under Session's short source borrow.
impl Session {
    /// Borrows the declaration API; general provider keyboard focus stays deferred.
    pub fn decorations_mut(&mut self) -> DecorationsMut<'_> {
        self.reconcile_decorations();
        DecorationsMut(self)
    }
    /// Lazily registers one renderer and invalidates it after a real definition edit.
    fn decoration_changed(&mut self) {
        if self.decorations.entries.is_empty() {
            // Empty annotation sets release the lazy provider and its retained strings.
            if let Some(id) = self.decorations.provider.take() {
                self.overlays.remove(id);
            }
        } else if self.decorations.provider.is_none() {
            self.decorations.provider = Some(self.overlays.add(
                Box::new(puzzle_overlay::decoration::render::DecorationsModel::default()),
                Box::new(puzzle_overlay::decoration::render::Decorations::default()),
            ));
        }
        // Coarse invalidation survives an external frame handoff just like parser damage.
        self.overlay_changed(EventKinds::DECORATIONS);
        self.needs_frame = true;
        self.services.waker.request_frame();
    }
    /// Drops stale source content even while it is outside the provider's visible view.
    pub(super) fn reconcile_decorations(&mut self) {
        let before = self.decorations.entries.len();
        self.decorations.entries.retain_mut(|entry| {
            if !entry.declaration.auto_remove {
                return true;
            }
            // VT's version witnesses recognize identical overwrites and preserve valid reflow.
            let gone = entry
                .declaration
                .anchor
                .sample(&self.terminal, self.overlays.pointer)
                == Resolved::Gone;
            let rewritten = entry
                .witness
                .as_mut()
                .is_some_and(|w| w.refresh(&self.terminal).is_none());
            if gone || rewritten {
                if let Some(witness) = entry.witness.take() {
                    witness.clear(&mut self.terminal);
                }
                return false;
            }
            true
        });
        if before != self.decorations.entries.len() {
            self.decoration_changed();
        }
    }
}
