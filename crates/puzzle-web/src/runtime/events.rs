//! Convenience events are delivered after the synchronous core turn.
//! Session's single result queue carries completed facts as owned values.
//! The host maps them to the existing JS subset after transport delivery;
//! no observer captures JS values or wraps them in an unsafe Send adapter.
//!
//! A title is text, a bell has no payload, and resize carries cell counts.
//! Those are the public convenience payloads, not arbitrary parser objects.
//! These parser facts plus theme changes are buffered; later M10 convenience events retain
//! their reserved names without creating hidden JS callback paths now.
//!
//! Subscription identities belong to the platform. Removing one callback
//! cannot remove the Session result queue or disturb another subscriber.
//!
//! Callbacks are copied out before invocation. The page can unsubscribe,
//! write output or dispose its terminal during delivery without a mutable
//! Session borrow spanning that call. Disposal stops further delivery.
//! A throwing event subscriber is logged; it cannot prevent another
//! subscriber from receiving the same already completed terminal fact.
//!
//! Facts are buffered as owned domain messages; subscribers run only after
//! Session's borrow has ended. Disposal removes registrations by identity.
use super::*;
use puzzle_term::messages::Event;

/// Delivers one convenience event without a borrow spanning foreign code.
pub(super) fn emit(owner: &Owner, name: &str, data: JsValue) {
    // Clone functions before calling foreign code, which may mutate this registration list.
    let callbacks: Vec<_> = owner
        .borrow()
        .events
        .iter()
        .filter(|(_, n, _)| n == name)
        .map(|(_, _, cb)| cb.clone())
        .collect();
    for callback in callbacks {
        // A preceding callback may have disposed this instance; remaining callbacks stop.
        if owner.borrow().disposed {
            break;
        }
        if let Err(error) = callback.call1(&JsValue::UNDEFINED, &data) {
            tracing::warn!(event = "web.event_callback_failed", ?error);
        }
    }
}

/// Terminal events retain their parser order before host callbacks run.
pub(super) fn flush(owner: &Owner) {
    loop {
        let event = {
            // Release the core borrow before converting or delivering the next fact.
            let mut r = owner.borrow_mut();
            r.take_fact()
        };
        match event {
            Some(Event::TitleChanged(title)) => emit(owner, "title", title.into()),
            Some(Event::Bell) => emit(owner, "bell", JsValue::UNDEFINED),
            Some(Event::GridResized { columns, rows }) => {
                // Expose cell counts instead of internal row identities or render dimensions.
                let object = js_sys::Object::new();
                let _ = js_sys::Reflect::set(&object, &"cols".into(), &(columns as u32).into());
                let _ = js_sys::Reflect::set(&object, &"rows".into(), &(rows as u32).into());
                emit(owner, "resize", object.into());
            }
            Some(Event::ThemeChanged { .. }) => {
                if let Err(error) = super::media::css(owner) {
                    emit(owner, "error", error);
                }
            }
            Some(Event::ViewportScrolled(px)) => {
                let object = js_sys::Object::new();
                let _ = js_sys::Reflect::set(&object, &"offsetPx".into(), &px.into());
                emit(owner, "scroll", object.into());
            }
            Some(Event::SelectionChanged(range)) => {
                if let Ok(data) = super::api_selection::range_value(range) {
                    emit(owner, "selectionChange", data);
                }
            }
            Some(Event::SearchRequested) => emit(owner, "searchRequested", JsValue::UNDEFINED),
            None => return,
            _ => {}
        }
    }
}

/// Link navigation remains synchronous with the DOM gesture and can be vetoed.
/// Event objects use the browser's own preventDefault semantics, avoiding a
/// callback-owned mutable core borrow or a closure that outlives its Session.
pub(super) fn allow_link(owner: &Owner, uri: &str) -> bool {
    let init = web_sys::EventInit::new();
    init.set_cancelable(true);
    let Ok(event) = web_sys::Event::new_with_event_init_dict("linkOpen", &init) else {
        return false;
    };
    // The URI has already passed the post-interceptor scheme/length policy.
    if js_sys::Reflect::set(event.as_ref(), &"url".into(), &uri.into()).is_err() {
        return false;
    }
    emit(owner, "linkOpen", event.clone().into());
    !event.default_prevented() && !owner.borrow().disposed
}
