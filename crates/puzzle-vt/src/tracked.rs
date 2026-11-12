//! Stable line identities and positions through row rotation (§6.3).
//!
//! Scrolling preserves a line's ID; eviction explicitly marks references
//! gone. This lets selection, image and overlay anchors share one rule.
//! Slots keep generation-checked handle identities after retirement, but a
//! separate dense list contains only Present anchors on each screen. Eviction
//! therefore costs the current live set, never old image/search capacity.
//! Swap-removing from that list updates its moved slot's reverse index; public
//! slot indices never move. Gone handles remain Gone until their owner releases
//! them, and inactive-screen anchors cannot be invalidated by another ring.

/// Owner-only gaps use the same movement engine without publication copies.
mod private;
pub use private::PrivateTrackedPos;

/// Absolute line identity, never reused while a terminal exists.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LineId(pub u64);

/// Opaque generation-checked handle into the tracked-position registry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TrackedPos {
    /// Index can be recycled after a selection or search ends.
    index: usize,
    /// Old handles must never resolve a newly occupied slot.
    generation: u64,
}

/// One reusable registry slot and its reuse generation.
#[derive(Clone, Copy, Debug)]
struct Slot {
    /// `None` denotes a released handle available for another client.
    state: Option<PositionState>,
    /// Incremented when a slot is released, before it is reused.
    generation: u64,
    /// Screens have independent line-ID domains; inactive anchors must survive.
    screen: crate::terminal::ActiveScreen,
    /// Offset in the screen's dense live list; released and Gone slots have none.
    live_index: Option<usize>,
}

/// A position still retained by the ring, or evicted from history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositionState {
    /// The referenced line and its column remain available.
    Present {
        /// Stable identity of the retained row.
        line: LineId,
        /// Column within that row.
        column: usize,
    },
    /// The referenced line was evicted and must not resolve again.
    Gone,
}

/// One movement engine for public markers and owner-only source gaps.
#[derive(Debug)]
pub struct TrackedPositions {
    /// Active positions change in place during scroll and reflow.
    positions: Vec<Slot>,
    /// Released indices are reused so repeated searches cannot grow forever.
    free: Vec<usize>,
    /// Only present anchors participate in eviction/reflow, never retired capacity.
    live: [Vec<usize>; 2],
    /// Lazy owner-only handles never enter a detached public snapshot.
    private: Option<Box<private::Arena>>,
    /// Ring operations affect this screen only, including offscreen resize work.
    active: crate::terminal::ActiveScreen,
}

/// Empty registries begin in the terminal's primary line-ID domain.
impl Default for TrackedPositions {
    /// No storage is allocated until the first live anchor.
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            private: None,
            free: Vec::new(),
            live: [Vec::new(), Vec::new()],
            active: crate::terminal::ActiveScreen::Primary,
        }
    }
}

impl TrackedPositions {
    /// Selects which independent ring a subsequent mutation belongs to.
    pub fn set_screen(&mut self, screen: crate::terminal::ActiveScreen) {
        self.active = screen;
        if let Some(arena) = &mut self.private {
            arena.positions.set_screen(screen);
        }
    }

    /// Copies public frame metadata while reusing warmed backing allocations.
    /// Clone uses this same path; private source ownership never enters a frame.
    pub fn snapshot_from(&mut self, source: &Self) {
        // Only a new metadata high-water mark can grow these owned buffers (P1).
        // Slot reverse indices and dense membership must describe the same copy;
        // providers may resolve handles from this detached publication afterward.
        // Detached consumers can resolve only public handles. Private source
        // storage remains on the Session owner and is never copied per frame.
        self.private = None;
        self.positions.clone_from(&source.positions);
        self.free.clone_from(&source.free);
        for (target, source) in self.live.iter_mut().zip(&source.live) {
            target.clone_from(source);
        }
        self.active = source.active;
    }

    /// Repositions retained anchors when logical lines acquire new row IDs.
    pub fn remap(&mut self, mut resolve: impl FnMut(LineId, usize) -> PositionState) {
        self.remap_positions(&mut resolve);
    }

    /// Both visibility domains borrow one callback type and the same update engine.
    fn remap_positions(&mut self, resolve: &mut impl FnMut(LineId, usize) -> PositionState) {
        // A handle stays stable even though its line and column may move.
        // Gone handles remain gone; reflow must never resurrect evicted text.
        if let Some(arena) = &mut self.private
            && arena.occupied != 0
        {
            arena.positions.remap_positions(resolve);
        }
        let screen = self.screen_index();
        let mut offset = 0;
        while offset < self.live[screen].len() {
            let index = self.live[screen][offset];
            let slot = &mut self.positions[index];
            if let Some(PositionState::Present { line, column }) = slot.state {
                slot.state = Some(resolve(line, column));
            }
            if slot.state == Some(PositionState::Gone) {
                // Removal moves the last live index here; visit that one next.
                self.remove_live(index);
            } else {
                offset += 1;
            }
        }
    }

    /// Creates a handle to a currently retained cell.
    pub fn track(&mut self, line: LineId, column: usize) -> TrackedPos {
        // Reuse a released index first. A search may create thousands of
        // endpoint handles; reissuing its query must not retain another
        // thousands of slots after the previous query was cleared.
        let screen = self.screen_index();
        let live_index = self.live[screen].len();
        // Primary and alternate line numbers can overlap. Membership follows the
        // owning screen, rather than inferring that ownership from a line's ID.
        let index = if let Some(index) = self.free.pop() {
            let slot = &mut self.positions[index];
            slot.state = Some(PositionState::Present { line, column });
            slot.screen = self.active;
            slot.live_index = Some(live_index);
            index
        } else {
            // Slot and live-list capacity grow only with simultaneous owners.
            let index = self.positions.len();
            self.positions.push(Slot {
                state: Some(PositionState::Present { line, column }),
                generation: 0,
                screen: self.active,
                live_index: Some(live_index),
            });
            index
        };
        self.live[screen].push(index);
        TrackedPos {
            index,
            generation: self.positions[index].generation,
        }
    }

    /// Resolves a handle without exposing the registry's storage.
    #[must_use]
    pub fn get(&self, handle: TrackedPos) -> PositionState {
        // The generation check distinguishes a released marker from an
        // unrelated selection or match later placed at the same index.
        self.positions
            .get(handle.index)
            .filter(|slot| slot.generation == handle.generation)
            .and_then(|slot| slot.state)
            .unwrap_or(PositionState::Gone)
    }

    /// Gone slots still belong to their owner; released generations do not.
    pub(crate) fn registered(&self, handle: TrackedPos) -> bool {
        self.positions
            .get(handle.index)
            .is_some_and(|slot| slot.generation == handle.generation && slot.state.is_some())
    }

    /// Reports retained slot capacity to the bounded-search regression.
    #[cfg(test)]
    pub(crate) fn positions_len(&self) -> usize {
        self.positions.len()
    }

    /// Releases an owner-held handle and invalidates all copied old handles.
    ///
    /// Searches can return thousands of matches, so their slots must be
    /// recycled when a query is replaced instead of accumulating forever.
    pub fn release(&mut self, handle: TrackedPos) {
        let Some(slot) = self.positions.get_mut(handle.index) else {
            return;
        };
        if slot.generation != handle.generation || slot.state.is_none() {
            return;
        }
        self.remove_live(handle.index);
        let slot = &mut self.positions[handle.index];
        slot.state = None;
        // A stale copy keeps the old generation. Even theoretical exhaustion
        // cannot authorize reuse of that same identity after retirement.
        // Exhausted slots stay retired forever. No old public or private handle
        // can alias a new owner after a generation wraps to its previous value.
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
            self.free.push(handle.index);
        }
    }

    /// Invalidates every position on an evicted line without allocating.
    pub fn evict(&mut self, line: LineId) {
        // Preserve the original empty-registry path: ordinary text scrolling
        // need not select a screen or look up a dense list when no owner exists.
        if let Some(arena) = &mut self.private
            && arena.occupied != 0
        {
            arena.positions.evict(line);
        }
        if self.positions.is_empty() {
            return;
        }
        let screen = self.screen_index();
        let mut offset = 0;
        while offset < self.live[screen].len() {
            #[cfg(test)]
            crate::scroll_operations::anchor_examined();
            let index = self.live[screen][offset];
            let slot = &mut self.positions[index];
            if matches!(slot.state, Some(PositionState::Present { line: id, .. }) if id == line) {
                slot.state = Some(PositionState::Gone);
                self.remove_live(index);
            } else {
                offset += 1;
            }
        }
    }

    /// Selects the independent primary/alternate anchor list.
    fn screen_index(&self) -> usize {
        usize::from(self.active == crate::terminal::ActiveScreen::Alternate)
    }

    /// Removes an anchor in constant time while preserving every public handle.
    fn remove_live(&mut self, index: usize) {
        let slot = &mut self.positions[index];
        let Some(offset) = slot.live_index.take() else {
            // A Gone position remains resolvable until its owner releases it.
            return;
        };
        let screen = usize::from(slot.screen == crate::terminal::ActiveScreen::Alternate);
        let live = &mut self.live[screen];
        live.swap_remove(offset);
        // The last member now occupies the removed offset. Repair its reverse
        // link so a subsequent release removes that member, not an unrelated one.
        if let Some(&moved) = live.get(offset) {
            self.positions[moved].live_index = Some(offset);
        }
    }
}

#[cfg(test)]
#[path = "tracked/tests.rs"]
mod tests;
