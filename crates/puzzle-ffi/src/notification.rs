//! Structured native notification callbacks and owner-local completion (M18.4).
//!
//! The host receives already-parsed options; it only implements native delivery.
//! Opaque generation tokens are copied during the call and cannot retain C storage.
//!
//! notification/show carries all supported options so the native host only maps
//! them to its OS presentation API. It does not reconstruct an OSC sequence.
//! notification/close identifies one delivery generation, not a reusable wire ID.
//! notification/focus is already authorized by a real user activation callback.
//! Callback buffers last through app_tick delivery; asynchronous hosts copy them.
//! Feedback first validates lengths and tags, then checks the surface capability.
//! The Session owner decides whether a well-formed token is still active.
//! A duplicate or stale callback is therefore inert after replacement or RIS.
//! No native posting or completion waits occur under this marshalling boundary.
use crate::{
    abi::{Bytes, ResultCode},
    memory, registry,
};
use puzzle_embed::{NotificationEffect as Effect, NotificationFeedback as Feedback};
use serde_json::{Value, json};
/// Converts host work to callback data; no OSC grammar is delegated to Swift.
pub(crate) fn event(effect: Effect) -> super::events::OwnedEvent {
    let (name, value) = match effect {
        Effect::Show {
            token,
            notification: n,
        } => {
            // Stable strings describe semantics; Swift need not depend on Rust enum layout.
            let occasion = match n.occasion {
                puzzle_embed::NotificationOccasion::Always => "always",
                puzzle_embed::NotificationOccasion::Unfocused => "unfocused",
                puzzle_embed::NotificationOccasion::Invisible => "invisible",
            };
            (
                "notification/show",
                json!({"token": token, "id": n.id,
                "title": n.title, "body": n.body, "urgency": n.urgency,
                "occasion": occasion, "actions": {"focus": n.focus, "report": n.report},
                "closeReport": n.close_report}),
            )
        }
        Effect::Close(token) => ("notification/close", json!({"token":token})),
        Effect::Focus => ("notification/focus", Value::Null),
    };
    // Owned JSON bytes survive unlocking the registry before the callback runs.
    (name.into(), value.to_string().into_bytes().into())
}
/// Reports user activation, dismissal, unsupported close tracking or delivery failure.
///
#[doc = "# Safety"]
/// `token` addresses readable UTF-8 for this call only; no pointer is retained.
#[allow(
    unsafe_code,
    reason = "D-43: checked call-scoped notification token copy"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_notification_feedback(
    id: u64,
    token: Bytes,
    outcome: u32,
) -> ResultCode {
    registry::result((|| {
        // Reject sizes/tags before accessing foreign data or constructing an enum.
        if token.len == 0 || token.len > 64 {
            return Err(ResultCode::InvalidArgument);
        }
        // Validate the ABI discriminant before it can reach lifecycle/report routing.
        let outcome = match outcome {
            0 => Feedback::Activated,
            1 => Feedback::Closed,
            2 => Feedback::Untracked,
            3 => Feedback::Failed,
            _ => return Err(ResultCode::InvalidArgument),
        };
        // SAFETY: caller supplies a live readable UTF-8 buffer; helper checks its range.
        let token = unsafe { memory::text(token)? };
        // Only handle metadata is locked; queuing feedback never borrows Session.
        registry::lock()?
            .surface(id)?
            .surface
            .notification_feedback(token, outcome)
            .map_err(ResultCode::from)
    })())
}
