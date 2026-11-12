//! Visibility and system preferences are facts, not user settings overrides.
//!
//! Auto appearance follows prefers-color-scheme through the same core path as
//! desktop ThemeChanged. A forced light/dark setting remains authoritative.
//! Reduced motion stops animation without changing palette selection.
//! Forced colors requests a system palette from the platform host separately.
//!
//! Visibility disables painting and blink timers, not incoming byte processing.
//! A returning page requests a repaint through its host, so old hidden frames
//! are never replayed. There is no polling of document.hidden in an idle loop.
//! Each callback is retained by a removable listener, with only weak state
//! ownership so removing a canvas also releases document/media registrations.
use crate::dom::{DomHost, KeyboardState, listener::Listener};
use puzzle_term::theme::Appearance;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::JsValue;
use web_sys::MediaQueryList;

/// Distinct preference lanes prevent boolean media matches being confused.
#[derive(Clone, Copy)]
enum Preference {
    /// A system appearance fact; the core still resolves auto versus forced mode.
    ColorScheme,
    /// Disables animation rather than selecting a different theme.
    ReducedMotion,
    /// Lets the host resolve CSS system colors for high-contrast display.
    ForcedColors,
}

/// Initial and subsequent media changes use the same host policy path.
/// Query matches are sampled from the retained MediaQueryList, not a cached
/// startup boolean. A page can remain open through multiple OS theme switches.
/// Each listener keeps its own query alive until terminal disposal.
pub(super) fn install<H: DomHost + 'static>(
    state: &Rc<RefCell<KeyboardState<H>>>,
) -> Result<Vec<Listener>, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("Missing browser window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("Missing document"))?;
    let weak = Rc::downgrade(state);
    // Document hidden state is read at event time, after the browser updated it.
    let page = document.clone();
    // Observe the document rather than canvas visibility: transports keep parsing.
    let mut listeners = vec![Listener::new(
        document.as_ref(),
        "visibilitychange",
        move |_| {
            if let Some(state) = weak.upgrade()
                && let Ok(mut state) = state.try_borrow_mut()
            {
                state.host.visibility(!page.hidden());
            }
        },
    )?];
    for (kind, query) in [
        (Preference::ColorScheme, "(prefers-color-scheme: dark)"),
        (
            Preference::ReducedMotion,
            "(prefers-reduced-motion: reduce)",
        ),
        (Preference::ForcedColors, "(forced-colors: active)"),
    ] {
        if let Some(list) = window.match_media(query)? {
            // An already-dark page must start in dark auto mode before first paint.
            apply(state, kind, &list);
            let weak = Rc::downgrade(state);
            let current = list.clone();
            // One query and one callback per preference; none wakes periodically.
            listeners.push(Listener::new(list.as_ref(), "change", move |_| {
                if let Some(state) = weak.upgrade() {
                    apply(&state, kind, &current);
                }
            })?);
        }
    }
    Ok(listeners)
}

/// Bool media state never overrides a forced light/dark setting in the core.
/// Passing system appearance through the host preserves OSC override clearing
/// and mode-2031 notifications when the resolved theme actually changes.
fn apply<H: DomHost>(
    state: &Rc<RefCell<KeyboardState<H>>>,
    kind: Preference,
    list: &MediaQueryList,
) {
    if let Ok(mut state) = state.try_borrow_mut() {
        // All three policies update synchronously; any repaint is scheduled in rAF.
        match kind {
            Preference::ColorScheme => state.host.appearance(if list.matches() {
                Appearance::Dark
            } else {
                Appearance::Light
            }),
            Preference::ReducedMotion => state.host.reduced_motion(list.matches()),
            Preference::ForcedColors => state.host.forced_colors(list.matches()),
        }
    }
}
