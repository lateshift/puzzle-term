//! Callback buffers own their bytes until each host-thread invocation returns.
//!
//! Facts use the core's stable wire catalogue. Native-only observations become
//! plain JSON without changing their policy. Transport input remains raw bytes.
//! Marker handles are translated once when their asynchronous result arrives;
//! source ownership remains with Session until the host removes the marker.
//!
//! Translation runs after App::tick has collected owned events and before any
//! callback begins. The registry lock protects only C capability metadata; no
//! Session, frame builder or native view is borrowed while callbacks execute.
//! A C event receives two pointer/length ranges backed by this owned vector.
//! Returning from the callback ends those borrows, so hosts copy what they need.
//!
//! Internal monotonic surface IDs map to external generation-checked IDs. The
//! mapping survives endpoint close until the driver's release notification, which
//! allows asynchronous hosts to correlate completion without resurrecting input.
//! Marker creation similarly returns a local C capability only when its owner has
//! answered. A response queued before close is discarded if its surface is gone.
//! The model still owns tracked positions and their reflow/eviction semantics.
//!
//! Core facts already implement ToWire and retain their catalogue type names.
//! This file adds only native observations such as clipboard transactions and
//! transport bytes. JSON serialization is confined to this native boundary;
//! neither the core nor the browser gains a JSON dependency from this API.
//! Transport input is raw because keys and replies can contain arbitrary bytes.
//! JSON integers carry handle identity without converting through floating point.
//! Optional protocol facts travel through the same core extension channel.
//! Render redraw requests are consumed inside App, not sent back as host timers.
//! Clipboard reads are host requests; the later answer follows the normal paste
//! policy rather than bypassing sanitation or the current bracketed-paste mode.
//! Cursor and binding observations contain no mutable Session references.
//! MIME requests are host effects, delivered on app_tick like legacy clipboard work.
//! Their callback data is owned until delivery and copied by any retaining host.
//! Correlation survives asynchronous OS completion without retaining native pointers.
//! A paste credential is deliberately absent from the MIME event's JSON payload.
//! Only the clipboard revision is disclosed so the host can reject changed data.
//! Closing a surface still suppresses all later callbacks, including clipboard work.
use crate::{abi::ResultCode, registry::Registry};
use puzzle_embed::{AppEvent, SurfaceAction, SurfaceId};
use puzzle_term::{
    HostEvent,
    messages::{ToWire, Value},
    platform::ClipboardEffect,
};
use serde_json::{Value as Json, json};

/// Owned callback payload; never a borrowed Session or native object.
pub(crate) type OwnedEvent = (String, payload::Payload);
/// Borrowed precharged replies do not allocate a second callback payload.
mod payload;
/// Converts one tick batch while registry metadata is checked on the owner thread.
pub(crate) fn translate(
    registry: &mut Registry,
    app: u64,
    events: Vec<(SurfaceId, AppEvent)>,
) -> Result<Vec<(u64, OwnedEvent)>, ResultCode> {
    // One owned batch survives unlocking the registry for callback reentrancy.
    let mut output = Vec::new();
    for (internal, event) in events {
        // Ignore effects from partial startup that never issued a C capability.
        let Some(id) = registry.app(app)?.identities.get(&internal).copied() else {
            continue;
        };
        let event = match event {
            AppEvent::SurfaceReleased => {
                // Completion retires the old identity mapping without enabling stale input.
                registry.app(app)?.identities.remove(&internal);
                Some(encode("surface/released", Json::Null))
            }
            AppEvent::Host(HostEvent::MarkerAdded { request, marker }) => {
                // A result queued before close cannot resurrect a closed endpoint.
                if let Ok(surface) = registry.surface(id) {
                    let marker = match marker {
                        Some(marker) => {
                            Some(surface.markers.insert(marker).map_err(|(error, _)| error)?)
                        }
                        None => None,
                    };
                    Some(encode(
                        "surface/markerAdded",
                        json!({"request": request, "marker": marker}),
                    ))
                } else {
                    None
                }
            }
            other => convert(other),
        };
        if let Some(event) = event {
            output.push((id, event));
        }
    }
    Ok(output)
}
/// The wire value tree has no serde dependency; only this native boundary needs JSON.
fn json_value(value: Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::Bool(v) => Json::Bool(v),
        Value::Number(v) => json!(v),
        Value::String(v) => Json::String(v),
        // Structured facts encode bytes as JSON arrays; transport/input bypasses JSON.
        Value::Bytes(v) => json!(v),
        Value::List(v) => Json::Array(v.into_iter().map(json_value).collect()),
        Value::Map(v) => Json::Object(v.into_iter().map(|(k, v)| (k, json_value(v))).collect()),
    }
}
/// JSON values cannot contain invalid floats after their typed owner has validated them.
fn encode(name: &str, value: Json) -> OwnedEvent {
    (name.into(), value.to_string().into_bytes().into())
}
/// Terminal facts preserve the core catalogue spelling and extension payload.
fn convert(event: AppEvent) -> Option<OwnedEvent> {
    match event {
        // The core catalogue controls public names and payload validation.
        AppEvent::Host(HostEvent::Fact(fact)) => match fact.to_value() {
            Ok(value) => Some(encode(fact.type_name(), json_value(value))),
            Err(error) => {
                tracing::error!(event = "ffi.event_translation_failed", ?error);
                None
            }
        },
        AppEvent::TransportInput(data) => Some(("transport/input".into(), data.into())),
        AppEvent::TransportReply(reply) => {
            if let Some(lease) = reply.bytes.lease() {
                return Some((
                    "transport/reply".into(),
                    payload::Payload::Precharged(lease),
                ));
            }
            // A binary receipt prefix avoids JSON/Base64 amplification of the
            // already charged payload. The C contract defines little-endian u64.
            let mut payload = Vec::with_capacity(8 + reply.bytes.len());
            payload.extend_from_slice(&reply.ticket.id().to_le_bytes());
            payload.extend_from_slice(&reply.bytes);
            Some(("transport/reply".into(), payload.into()))
        }
        // Preserve coalesced grid and device-pixel sizes for host-owned sources.
        AppEvent::TransportResize(size) => Some(encode(
            "transport/resize",
            json!({"cols": size.columns, "rows": size.rows, "width": size.pixel_width, "height": size.pixel_height}),
        )),
        AppEvent::TransportClose => Some(encode("transport/close", Json::Null)),
        // Link scheme and modifier-click policy have already run on Session.
        AppEvent::OpenLink(uri) => Some(encode("link/open", json!({"uri": uri}))),
        // Notifications have already passed the shared rate and control-character limits.
        AppEvent::DesktopNotification(effect) => Some(super::notification::event(effect)),
        AppEvent::Notification { title, body } => Some(encode(
            "notification/show",
            json!({"title": title, "body": body}),
        )),
        AppEvent::Render(SurfaceAction::Close) => {
            Some(encode("surface/closeRequested", Json::Null))
        }
        AppEvent::Render(SurfaceAction::FlushPointer) => {
            Some(encode("surface/flushPointer", Json::Null))
        }
        // Native presentation facts remain available even with frame statistics disabled.
        AppEvent::Render(SurfaceAction::FirstFrame) => {
            Some(encode("surface/firstFrame", Json::Null))
        }
        AppEvent::Render(SurfaceAction::CursorRect(rect)) => {
            Some(encode("surface/cursorRect", json!(rect)))
        }
        // Redraws have already reached the actor inside App::tick; hosts need no second scheduler.
        AppEvent::Render(_) => None,
        AppEvent::Host(event) => host(event),
        // Release and markers require checked registry translation in the outer function.
        AppEvent::SurfaceReleased => None,
    }
}
/// Native host facts stay data, while internal scheduling never leaks to C callbacks.
fn host(event: HostEvent) -> Option<OwnedEvent> {
    let (name, payload) = match event {
        // Copied presentation metadata shares one small JSON-only encoder.
        HostEvent::Colors { .. } | HostEvent::SearchState(_) | HostEvent::InputState(_) => {
            return host_state(event);
        }
        HostEvent::Activity => ("surface/activity", Json::Null),
        HostEvent::SearchRequested => ("search/requested", Json::Null),
        HostEvent::SelectedText { request, text } => {
            ("selection/text", json!({"request":request,"text":text}))
        }
        // The optional owner defines its documented binary framing and accounts
        // the Arc allocation. Translation never copies the source text again.
        HostEvent::OscEffect(effect) => {
            return Some((effect.kind.into(), payload::Payload::Osc(effect.data)));
        }
        HostEvent::Drop(effect) => return Some(super::dnd::event(effect)),
        HostEvent::Clipboard(ClipboardEffect::Mime(transaction)) => {
            return Some(super::clipboard::event(transaction));
        }
        HostEvent::Clipboard(ClipboardEffect::Read) => ("clipboard/read", Json::Null),
        HostEvent::Clipboard(ClipboardEffect::Write(text)) => {
            ("clipboard/write", json!({"text": text}))
        }
        HostEvent::Clipboard(ClipboardEffect::WriteHtml { html, text }) => {
            ("clipboard/write", json!({"html": html, "text": text}))
        }
        HostEvent::OpenLink(uri) => ("link/open", json!({"uri": uri})),
        HostEvent::NativeTextAllowed(allowed) => ("input/textAllowed", json!(allowed)),
        HostEvent::PointerHandled(handled) => ("input/pointerHandled", json!(handled)),
        HostEvent::CursorChanged(cursor) => ("pointer/shape", json!(format!("{cursor:?}"))),
        // A rejected declaration keeps its original correlation token and validation reason.
        HostEvent::DecorationAdded { request, result } => (
            "surface/decorationAdded",
            match result {
                Ok(id) => json!({"request": request, "id": id.0}),
                Err(error) => json!({"request": request, "error": error}),
            },
        ),
        // Host-level actions retain the store-wide target flag and appearance observation.
        HostEvent::BoundAction { action, all, dark } => (
            "binding/action",
            json!({"name": action.info().name, "action": format!("{action:?}"), "all": all, "dark": dark}),
        ),
        // App routes these internally; no host callback can schedule a Session directly.
        _ => return None,
    };
    Some(encode(name, payload))
}

#[cfg(test)]
/// Binary callback ownership never passes through JSON or a second text copy.
mod tests;

/// Encode copied chrome/search/input observations without re-entering an owner.
fn host_state(event: HostEvent) -> Option<OwnedEvent> {
    let (name, payload) = match event {
        // RGB is already encoded sRGB; serialization must not transfer it again.
        // Dark follows paper luminance, independently of the theme's label.
        HostEvent::Colors {
            background,
            foreground,
            dark,
        } => (
            "surface/colors",
            json!({"background": background, "foreground": foreground, "dark": dark}),
        ),
        // Search counts include completion, not merely matches on the current screen.
        HostEvent::SearchState(state) => (
            "search/state",
            json!({"count": state.count, "current": state.current, "complete": state.complete, "scanned": state.scanned}),
        ),
        // Metrics arrive as values; no host can borrow a Session through this event.
        HostEvent::InputState(state) => (
            "surface/inputState",
            json!({"cols": state.grid[0], "rows": state.grid[1], "cellWidth": state.cell.map(|c| c.width_px), "cellHeight": state.cell.map(|c| c.height_px)}),
        ),
        // The caller routes only these three metadata kinds here.
        _ => return None,
    };
    // Owned JSON survives until the call-scoped C callback borrows its bytes.
    Some(encode(name, payload))
}
