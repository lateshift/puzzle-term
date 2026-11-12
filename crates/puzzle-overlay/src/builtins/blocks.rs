//! OSC command gutters and copy affordances use the public split overlay API.
//!
//! Pointer state belongs to the model. A presented full-row observer lets a
//! hover reveal the copy button without consuming selection or mouse reports.
//! The button occupies Top; only its left press consumes input. Its action
//! carries an identity, not extracted text, so normal copy policy runs later.
//! The view clips intervals to current visible geometry and uses semantic tokens.
//! Duration is final owner data; there is no polling timer for a running command.
//!
//! Output-row observers live below text and never consume ordinary selection.
//! The small copy button lives in Top, where only a left press is handled.
//! Hover identity is the command ID, so pointer movement within its output
//! cannot redirect a button to whichever command happened to finish last.
//! Completed blocks use zoneOk/zoneError tokens from the published theme.
//! Unknown exit status remains visually neutral through the success token.
//! Long commands receive a final duration label; short commands have no badge.
//! The view has no clipboard access and never traverses retained scrollback.
//! Its model action is converted to a normal copy request by the Session owner.
//! A policy veto therefore works exactly as it does for a keyboard copy.
//! Copying cannot replace an unrelated selection because it carries an ID.
//! Every hit bound comes from the draw list that will actually be presented.
use super::*;
use crate::{
    draw::{CursorIcon, RegionId},
    model::{ModelCtx, OverlayModel, SharedState, ViewState, state_pool::StatePool},
    provider::{EventKinds, Handled, PointerEvent, PointerKind},
};
/// Render-owned cached primitives are separate from Session interaction state.
mod view;
pub use view::BlocksView;
/// Post-policy receipts use the same model clock as all overlay animation.
mod feedback;
pub use feedback::CopyFeedback;
use feedback::{FLASH_SECONDS, LABEL_SECONDS};
/// Frozen interaction and finite copy feedback, never a duplicate source model.
#[derive(Clone, Default)]
struct State {
    /// Hover is a stable OSC command identity.
    hover: Option<u64>,
    /// One explicit click replaces the preceding finite receipt.
    feedback: Option<CopyFeedback>,
}
/// High bit separates a button hit from its passive output-row observer.
const COPY: u64 = 1 << 63;
/// Interaction state is a stable command identity, not a duplicated zone model.
#[derive(Default)]
pub struct Blocks {
    /// Hover and finite receipts stay with the Session's interaction model.
    state: State,
    /// Immutable values recycle after readers release earlier publications.
    states: StatePool<State>,
}
/// Session-owned hover values are published independently of render-side caches.
impl OverlayModel for Blocks {
    /// Stable name carried by presented region tables and diagnostics.
    fn name(&self) -> &'static str {
        "command-blocks"
    }
    /// Positions/status follow output, reflow and viewport changes; hover follows input.
    fn wants(&self) -> EventKinds {
        EventKinds::ALL
    }
    /// Eviction invalidates hover without clamping it onto an unrelated new command.
    fn update(&mut self, ctx: &ModelCtx<'_>) {
        if self
            .state
            .hover
            .is_some_and(|id| !ctx.models.blocks.iter().any(|b| b.id == id))
        {
            self.state.hover = None;
        }
        if self.state.feedback.is_some_and(|receipt| {
            ctx.time >= receipt.started + LABEL_SECONDS
                || !ctx.models.blocks.iter().any(|block| block.id == receipt.id)
        }) {
            // Retiring the state removes both retained label and tint in one build.
            self.state.feedback = None;
        }
        if let Some(receipt) = self.state.feedback {
            ctx.wake_at(receipt.started + LABEL_SECONDS);
            if receipt.flash(ctx.time, ctx.reduced_motion) > 0.0 {
                ctx.animate_until(receipt.started + FLASH_SECONDS);
                ctx.wake_at(receipt.started + FLASH_SECONDS);
            }
        }
    }
    /// A clean model publishes nothing through the registry (P11).
    fn view_state(&mut self) -> SharedState {
        self.states.publish(&self.state)
    }
    /// A receipt is installed only after both copy-policy boundaries have drained.
    fn accept_state(&mut self, state: SharedState) -> bool {
        let Some(receipt) = state.as_ref().as_any().downcast_ref::<CopyFeedback>() else {
            return false;
        };
        self.state.feedback = Some(*receipt);
        true
    }
    /// Only an actual button press requests copy; hovering keeps local selection intact.
    fn on_pointer(
        &mut self,
        event: &PointerEvent,
        region: RegionId,
        ctx: &mut ModelCtx<'_>,
    ) -> Handled {
        // The high bit namespaces button regions without changing the command identity.
        let id = region.0 & !COPY;
        // Leaving a region removes hover; entering its button restores the same identity.
        if event.kind == PointerKind::Leave {
            self.state.hover = None;
        } else if ctx.models.blocks.iter().any(|block| block.id == id) {
            self.state.hover = Some(id);
        }
        // A stale or evicted block cannot remain an actionable copy target.
        if event.kind == PointerKind::Press
            && event.button == Some(0)
            && region.0 & COPY != 0
            && self.state.hover == Some(id)
        {
            // Extraction and ClipboardWrite policy run after the callback borrow ends.
            ctx.requests
                .action
                .set(Some(crate::models::Action::CopyZoneOutput(id)));
            return Handled::Yes;
        }
        Handled::No
    }
}
