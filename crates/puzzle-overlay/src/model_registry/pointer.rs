//! Hover transitions and captured drags are identical on native and web shells.
//!
//! A handled press captures its provider/region until release, including moves
//! outside its original rectangle. Capture changes input ownership rather than
//! painter order: hover still follows the top visible region. Removing a captured
//! provider releases it without sending input to a different provider (§11.4).
//!
//! Pointer coordinates are logical surface pixels, including padding. The
//! shells convert signed grid-device samples once, before this routing lane.
//! Picking uses the last actually presented region table;
//! input does not shape text or wait for another presentation.
//!
//! The hover identity consists of provider ID and region ID. A cursor-only
//! update does not emit another enter, but replacing the owner emits one
//! leave followed by one enter in that order. Slot priority is determined
//! by the picker, not by provider callback return values.
//!
//! Capture begins only for an explicitly handled press. Thereafter its
//! paired input cannot fall through to local selection or application
//! reporting just because the pointer moved out of the rectangle. Release
//! ends capture after delivering its callback, even when that callback says No.
//!
//! Model removals invalidate capture by stable registration identity.
//! A newly registered provider cannot inherit an old numeric vector index.
//! Leave clears surface hover but keeps a drag's paired-release ownership;
//! this mirrors DOM pointer capture and native out-of-window button events.
use super::*;
use crate::draw::{CursorIcon, RegionId};
use crate::model::ModelCtx;

/// One region identity with its last sampled cursor policy.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Target {
    /// Registration IDs are never reused when providers are removed.
    provider: ProviderId,
    /// Provider-local identity is stable across retained-list rebuilds.
    region: RegionId,
    /// The visible region supplies the shell's cursor policy.
    cursor: CursorIcon,
}
/// The registry owns input state even without lending callbacks outside Session.
#[derive(Default)]
pub(super) struct Routing {
    /// Last visible owner receives one leave before another receives enter.
    hover: Option<Target>,
    /// A handled press owns its paired moves/release outside the original region.
    capture: Option<Target>,
}

impl ModelRegistry {
    /// Routes a logical event before selection/reporting; callbacks stay with the Session owner.
    pub fn route_pointer(&mut self, event: &PointerEvent, ctx: &mut ModelCtx<'_>) -> PointerResult {
        // Invalid coordinates cannot become a captured application gesture or shader position.
        if event.point.iter().any(|v| !v.is_finite()) {
            return PointerResult::default();
        }
        self.pointer = (event.kind != PointerKind::Leave).then_some(event.point);
        let hit = self.pointer.and_then(|point| self.regions.hit(point)).map(
            |(provider, region, cursor)| Target {
                provider,
                region,
                cursor,
            },
        );
        self.transition(hit, event, ctx);
        // A removed provider cannot retain capture or hand that capture to its replacement.
        if self
            .routing
            .capture
            .is_some_and(|target| !self.exists(target))
        {
            self.routing.capture = None;
        }
        let target = self.routing.capture.or(hit);
        let handled = !matches!(event.kind, PointerKind::Enter | PointerKind::Leave)
            && target.is_some_and(|target| {
                self.on_pointer(target.provider, target.region, event, ctx) == Handled::Yes
            });
        if event.kind == PointerKind::Press && handled {
            self.routing.capture = target;
        }
        // A paired release remains consumed even if a provider returns No after ending its drag.
        let owned = self.routing.capture.is_some();
        if event.kind == PointerKind::Release {
            self.routing.capture = None;
        }
        // Release restores the actual hovered cursor immediately, including outside the surface.
        let cursor = self
            .routing
            .capture
            .or(hit)
            .map_or(CursorIcon::Default, |t| t.cursor);
        PointerResult {
            handled: handled || owned,
            cursor,
        }
    }

    /// Delivers leave/enter once when the top visible region identity changes.
    fn transition(&mut self, hit: Option<Target>, event: &PointerEvent, ctx: &mut ModelCtx<'_>) {
        let same = self
            .routing
            .hover
            .zip(hit)
            .is_some_and(|(a, b)| a.provider == b.provider && a.region == b.region);
        if same || (self.routing.hover.is_none() && hit.is_none()) {
            // Cursor updates can change without manufacturing another enter transition.
            self.routing.hover = hit;
            return;
        }
        if let Some(old) = self.routing.hover.take() {
            self.on_pointer(
                old.provider,
                old.region,
                &PointerEvent {
                    kind: PointerKind::Leave,
                    ..*event
                },
                ctx,
            );
        }
        self.routing.hover = hit;
        if let Some(new) = hit {
            self.on_pointer(
                new.provider,
                new.region,
                &PointerEvent {
                    kind: PointerKind::Enter,
                    ..*event
                },
                ctx,
            );
        }
        ctx.request_frame();
    }

    /// Capture validation uses retained registration identity, not a stale numeric index.
    fn exists(&self, target: Target) -> bool {
        self.entries.iter().any(|entry| entry.id == target.provider)
    }
}
