//! Session-owned models and dirty-state publication into rotating snapshots.
//!
//! Every registration starts dirty. An unchanged model keeps the same immutable
//! state and version; rotating slots copy only values whose version changed.
//! Views are moved once, never lent back by a renderer, even if a publication
//! is skipped. The renderer must accept registrations before returning a slot.
//!
//! A registration has two different lifetimes. The mutable model stays here
//! until removal; the paired view leaves once in a snapshot and thereafter
//! belongs to the renderer. Removing a model makes old presented regions inert
//! immediately and removes its view from the next selected snapshot's identity
//! set. Numeric IDs are never reused, so a captured drag cannot change owner.
//!
//! Scheduling runs on the injected Session clock. Each update consumes only its
//! own frame/until/wake requests; unioning those requests happens after callback
//! execution. This prevents an animated provider from waking a clean neighbour.
//! A deadline needs one final model update to publish the animation's endpoint.
//! Pointer input can dirty a model while a renderer holds an older immutable
//! state, without changing either that frame's ink or its capture ownership.
//!
//! Snapshot publication compares each entry's version against that particular
//! slot. Unchanged entries keep their shared payload, including across slot
//! rotations; there is no per-scalar or per-frame deep state copy (P11).
use crate::{model::*, provider::*, registry::ProviderId, view::OverlayView};
/// Pointer capture and hover belong to the same owner as interaction state.
mod pointer;

/// One interaction owner and the latest immutable value it published.
struct Entry {
    /// Monotonic identity prevents stale regions from reaching a replacement.
    id: ProviderId,
    /// All mutable provider state remains in this callback owner.
    model: Box<dyn OverlayModel>,
    /// A newly registered view moves once in a published slot.
    view: Option<Box<dyn OverlayView>>,
    /// Retained immutable payload; None until the initial dirty update.
    state: Option<SharedState>,
    /// Distinguishes changed payloads when each slot has a different history.
    version: u64,
    /// Source changes and pointer callbacks invalidate this model only.
    dirty: bool,
    /// Animation remains active until its endpoint has been published.
    until: f64,
    /// Future one-shot update without intervening frames.
    wake: f64,
}
/// Session-side provider registry, with no draw lists or font resources.
#[derive(Default)]
pub struct ModelRegistry {
    /// Registration order also determines painter ties in views.
    entries: Vec<Entry>,
    /// Removed IDs are never reused within a Session.
    next: u64,
    /// Input ownership survives builds, skipped publications and resize.
    routing: pointer::Routing,
    /// Last presented regions, independent of any newer unpresented build.
    regions: crate::regions::PresentedRegions,
    /// Logical pointer position for source-side anchor sampling.
    pub pointer: Option<[f32; 2]>,
}
/// Registration, invalidation and clocks affect only Session-owned state.
impl ModelRegistry {
    /// Registers the paired halves; the view moves to its independent owner on publication.
    pub fn add(&mut self, model: Box<dyn OverlayModel>, view: Box<dyn OverlayView>) -> ProviderId {
        let id = ProviderId(self.next);
        self.next = self.next.saturating_add(1);
        self.entries.push(Entry {
            id,
            model,
            view: Some(view),
            state: None,
            version: 0,
            dirty: true,
            until: 0.0,
            wake: 0.0,
        });
        id
    }
    /// Registers a draw-only view with a stateless, Session-owned invalidation model.
    pub fn add_view(
        &mut self,
        name: &'static str,
        wants: EventKinds,
        view: Box<dyn OverlayView>,
    ) -> ProviderId {
        self.add(Box::new(StaticModel::new(name, wants)), view)
    }
    /// Removes interaction immediately; the next snapshot removes retained drawing.
    pub fn remove(&mut self, id: ProviderId) {
        self.entries.retain(|entry| entry.id != id);
    }
    /// Invalidates only models interested in the coarse source event (P11).
    pub fn notify(&mut self, kind: EventKinds) {
        for entry in &mut self.entries {
            entry.dirty |= entry.model.wants().intersects(kind);
        }
    }
    /// Marks one model after an API edit rather than synthesizing a bus event.
    pub fn invalidate(&mut self, id: ProviderId) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            entry.dirty = true;
        }
    }
    /// Transfers immutable source state to its registered model adapter. Only
    /// that model can accept the type; views never mutate or schedule from it.
    pub fn replace_state(&mut self, id: ProviderId, state: SharedState) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        // A source publication does not invoke drawing or schedule a timer here.
        // The addressed model alone decides whether this immutable type belongs to it.
        if entry.model.accept_state(state) {
            // The next model update versions this value for every rotating slot.
            entry.dirty = true;
            true
        } else {
            false
        }
    }
    /// Empty registries need neither model updates nor overlay snapshot work.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Tests flags and deadlines without invoking any callback.
    pub fn needs_build(&self, time: f64) -> bool {
        self.entries.iter().any(|entry| due(entry, time))
    }
    /// Earliest one-shot update, independent of renderer availability.
    pub fn next_wake(&self) -> Option<f64> {
        self.entries
            .iter()
            .filter_map(|entry| (entry.wake > 0.0).then_some(entry.wake))
            .min_by(f64::total_cmp)
    }
    /// Runs dirty models and aggregates their scheduling requests independently.
    pub fn update(&mut self, ctx: &ModelCtx<'_>) {
        let mut requested = false;
        for entry in &mut self.entries {
            if !due(entry, ctx.time) {
                continue;
            }
            // Reset callback-local requests; one model cannot animate a neighbour.
            ctx.requests.frame.set(false);
            ctx.requests.until.set(0.0);
            ctx.requests.wake.set(0.0);
            entry.model.update(ctx);
            entry.state = Some(entry.model.view_state());
            entry.version = entry.version.wrapping_add(1);
            entry.until = ctx.requests.until.get();
            entry.wake = ctx.requests.wake.get();
            // Keep a final dirty update after the animation crosses its endpoint.
            entry.dirty = entry.until > ctx.time || ctx.requests.frame.get();
            requested |= ctx.requests.frame.get();
        }
        ctx.requests.frame.set(requested);
    }
    /// Updates a caller-owned slot without cloning an unchanged model's state.
    pub fn publish(&mut self, snapshot: &mut crate::view_registry::OverlaySnapshot) {
        snapshot
            .states
            .retain(|state| self.entries.iter().any(|entry| entry.id == state.id));
        for entry in &mut self.entries {
            if let Some(view) = entry.view.take() {
                // Registrations are consumed even from skipped slots, before recycling.
                snapshot.views.push((entry.id, view));
            }
            let Some(state) = &entry.state else {
                continue;
            };
            if let Some(previous) = snapshot
                .states
                .iter_mut()
                .find(|state| state.id == entry.id)
            {
                if previous.version != entry.version {
                    previous.state = state.clone();
                    previous.version = entry.version;
                }
            } else {
                snapshot.states.push(crate::view_registry::State {
                    id: entry.id,
                    version: entry.version,
                    state: state.clone(),
                });
            }
        }
    }
    /// Commits regions only after successful presentation, never merely after building.
    pub fn presented(
        &mut self,
        regions: crate::regions::PresentedRegions,
    ) -> crate::regions::PresentedRegions {
        // Return the previous table so hosts can recycle its warmed allocations.
        std::mem::replace(&mut self.regions, regions)
    }
    /// Invokes only the addressed interaction owner and collects its dirty requests.
    pub fn on_pointer(
        &mut self,
        id: ProviderId,
        region: crate::draw::RegionId,
        event: &PointerEvent,
        ctx: &mut ModelCtx<'_>,
    ) -> Handled {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return Handled::No;
        };
        let result = entry.model.on_pointer(event, region, ctx);
        entry.dirty |=
            entry.model.wants().intersects(EventKinds::POINTER) || ctx.requests.frame.get();
        result
    }
}
/// A sleeping deadline becomes work once, while animation includes its final state.
fn due(entry: &Entry, time: f64) -> bool {
    entry.dirty || entry.until > time || (entry.wake > 0.0 && entry.wake <= time)
}
