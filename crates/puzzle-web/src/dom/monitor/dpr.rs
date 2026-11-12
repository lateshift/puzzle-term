//! One re-armed resolution query detects monitor moves and browser zoom.
//!
//! A query for the old DPR changes only once. It must be replaced with a query
//! for the new DPR so a later monitor/zoom change is also detected in Safari,
//! where ResizeObserver's CSS content box may not have changed at all.
//! Replacing after a microtask keeps the executing callback alive until return.
//!
//! A monitor move can double backing pixels without changing CSS width/height.
//! Consequently a CSS-box ResizeObserver alone is insufficient on Safari.
//! The resolution callback reports the container at the new physical scale;
//! the core will recompute metrics and fit the grid through the host resize lane.
//!
//! This callback owns no timer and requests no refresh on an unchanged page.
//! The only microtask is scheduled in response to a real resolution change.
//! It rebuilds the query using the current ratio rather than the event's old
//! matches value, so successive monitor moves cannot leave it permanently false.
//!
//! State and watcher references are both weak inside deferred work. Disposal
//! removes the active registration and makes either weak upgrade fail; a late
//! resolution event therefore cannot recreate listeners for a removed terminal.
//! Container handles are retained only until that one queued turn finishes.
use crate::dom::{DomHost, KeyboardState, listener::Listener};
use js_sys::Promise;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::HtmlElement;

/// Retains only the currently armed query; disposal removes it immediately.
pub(super) struct DprWatch {
    /// Listener callback holds a weak reference here, preventing an owner cycle.
    current: Rc<RefCell<Option<Listener>>>,
}

/// No interval is needed: browser resolution changes drive the update.
impl DprWatch {
    /// Arms the initial DPR without issuing a redundant geometry update.
    pub(super) fn new<H: DomHost + 'static>(
        state: &Rc<RefCell<KeyboardState<H>>>,
        container: &HtmlElement,
    ) -> Result<Self, JsValue> {
        let current = Rc::new(RefCell::new(None));
        arm(state, container, &current)?;
        Ok(Self { current })
    }
}

/// Removes the listener even if a microtask still holds a temporary reference.
impl Drop for DprWatch {
    /// The queued re-arm upgrades a weak owner and therefore cannot resurrect it.
    fn drop(&mut self) {
        self.current.borrow_mut().take();
    }
}

/// Captures the present ratio; every transition reports geometry before re-arming.
fn arm<H: DomHost + 'static>(
    state: &Rc<RefCell<KeyboardState<H>>>,
    container: &HtmlElement,
    current: &Rc<RefCell<Option<Listener>>>,
) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("Missing browser window"))?;
    let Some(list) = window.match_media(&format!(
        "(resolution: {}dppx)",
        window.device_pixel_ratio()
    ))?
    else {
        return Ok(());
    };
    let weak_state = Rc::downgrade(state);
    let weak_current = Rc::downgrade(current);
    let container = container.clone();
    let listener = Listener::new(list.as_ref(), "change", move |_| {
        // A removed terminal cannot receive this event or its queued re-arm.
        let Some(state) = weak_state.upgrade() else {
            return;
        };
        if let Some(window) = web_sys::window()
            && let Ok(mut state) = state.try_borrow_mut()
        {
            let dpr = window.device_pixel_ratio();
            let rect = container.get_bounding_client_rect();
            state.host.resize(
                [
                    (rect.width() * dpr).round() as u32,
                    (rect.height() * dpr).round() as u32,
                ],
                dpr,
            );
        }
        let weak_state = weak_state.clone();
        let weak_current = weak_current.clone();
        let container = container.clone();
        wasm_bindgen_futures::spawn_local(async move {
            // Drop the old closure only after its active JS invocation returns.
            let _ = JsFuture::from(Promise::resolve(&JsValue::UNDEFINED)).await;
            if let (Some(state), Some(current)) = (weak_state.upgrade(), weak_current.upgrade())
                && arm(&state, &container, &current).is_err()
            {
                tracing::warn!(event = "web.dpr_watch_failed", "Resolution query rejected");
            }
        });
    })?;
    *current.borrow_mut() = Some(listener);
    Ok(())
}
