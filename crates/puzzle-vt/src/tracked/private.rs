//! Owner-only tracked positions share movement rules without entering snapshots.
//! Source consumers need stable gaps, but render-side providers never receive
//! those private handles. A separate lazy arena reuses the same tracker engine;
//! publication copies only the existing public arena. No protocol name, thread,
//! lock or second reflow/eviction algorithm is introduced.
//!
//! Slot capacity remains retained and measurable after the last private owner
//! releases it. Reusing that pool preserves slot generations and allocation
//! reuse; discarding and recreating it could resurrect a copied old handle.
//! An empty arena has no eviction/remap visits, and a never-used arena owns no
//! allocation. The optional consumer must account its actual retained capacity.
use super::{LineId, PositionState, TrackedPos, TrackedPositions};

/// An owner-only handle cannot be passed to public decoration/selection APIs.
/// Like a public TrackedPos, it is meaningful only in its issuing Terminal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivateTrackedPos(TrackedPos);

/// One private pool, using the same slot/live-list machinery as published markers.
#[derive(Debug)]
pub(crate) struct Arena {
    /// This inner engine never creates another private arena.
    pub(crate) positions: TrackedPositions,
    /// Present and Gone slots both retain ownership until explicit release.
    pub(crate) occupied: usize,
}

/// Private acquisition/query/release stays on the singular source owner.
impl TrackedPositions {
    /// Captures an owner-only position; publication never clones this arena.
    pub fn track_private(&mut self, line: LineId, column: usize) -> PrivateTrackedPos {
        let active = self.active;
        // Default leaves the nested engine's private arena absent: only this
        // top-level owner creates private handles, so storage cannot nest again.
        let arena = self.private.get_or_insert_with(|| {
            Box::new(Arena {
                positions: TrackedPositions {
                    active,
                    ..TrackedPositions::default()
                },
                occupied: 0,
            })
        });
        let handle = arena.positions.track(line, column);
        // Reserve release slots at acquisition so retiring an endpoint cannot
        // unexpectedly allocate after its admission footprint was checked.
        let capacity = arena.positions.positions.capacity();
        arena
            .positions
            .free
            .reserve(capacity.saturating_sub(arena.positions.free.len()));
        arena.occupied += 1;
        PrivateTrackedPos(handle)
    }

    /// Resolves only a private handle; public snapshots always return Gone.
    pub fn get_private(&self, handle: PrivateTrackedPos) -> PositionState {
        self.private
            .as_ref()
            .map_or(PositionState::Gone, |arena| arena.positions.get(handle.0))
    }

    /// Releases a generation-checked private slot without touching public markers.
    pub fn release_private(&mut self, handle: PrivateTrackedPos) {
        let Some(arena) = &mut self.private else {
            return;
        };
        let live = arena
            .positions
            .positions
            .get(handle.0.index)
            .is_some_and(|slot| slot.generation == handle.0.generation && slot.state.is_some());
        if live {
            // An evicted/Gone handle is still owned. Dense live membership is
            // already gone, but its slot must be retired exactly once here.
            arena.positions.release(handle.0);
            arena.occupied -= 1;
        }
    }

    /// Actual private pool capacity, including slack and the boxed arena header.
    /// Existing public selection/search storage is not charged to this owner.
    pub fn private_storage_bytes(&self) -> usize {
        self.private.as_ref().map_or(0, |arena| {
            size_of::<Arena>()
                + arena.positions.positions.capacity() * size_of::<super::Slot>()
                + (arena.positions.free.capacity()
                    + arena
                        .positions
                        .live
                        .iter()
                        .map(Vec::capacity)
                        .sum::<usize>())
                    * size_of::<usize>()
        })
    }
}

/// Cloning represents detached public metadata, never another private source owner.
impl Clone for TrackedPositions {
    /// Reuse the same publication path as the three rotating FrameSnapshot slots.
    fn clone(&self) -> Self {
        let mut snapshot = Self::default();
        snapshot.snapshot_from(self);
        snapshot
    }
    /// Existing public allocations are reused; private handles cannot cross the seam.
    fn clone_from(&mut self, source: &Self) {
        self.snapshot_from(source);
    }
}

/// Transactional capacity planning is cold and never runs in the print loop.
mod capacity;
