//! Container pixels and browser visibility/media changes (§12.2).
//!
//! Observer callbacks deliver copied facts to the host, never render a frame.
//! The host coalesces size changes until rAF and throttles PTY resize separately.
//! Native device-pixel content boxes avoid fractional-DPR rounding ambiguity;
//! Safari falls back to contentRect multiplied by the current DPR once.
//!
//! The observer watches the container, not the canvas's intrinsic backing size.
//! Changing canvas.width must not create a feedback loop that grows the host.
//! The host's CSS layout determines the container dimensions; the renderer
//! receives their physical counterpart for its next surface configuration.
//!
//! ResizeObserver entries may omit the newer physical content-box property.
//! Reflect checks that it is an array before indexing it; missing values fall
//! back to contentRect rather than attempting to index JavaScript undefined.
//! Both paths round to whole device pixels before crossing the platform seam.
//!
//! Visibility and media changes are independent. Hidden pages can still receive
//! theme and transport updates, but their host must stop rendering/blink wakes.
//! Returning to a visible page requests current content, not a stale saved frame.
//! Appearance is a system fact: a user-forced theme remains the core's decision.
//!
//! Reduced motion controls animation policy and is not an implicit theme switch.
//! Forced colors likewise uses its own host callback, allowing actual system
//! palette sampling without placing browser CSS values into the neutral core.
//! The same callbacks handle initial state and later media-query transitions.
use super::{DomHost, KeyboardState, listener::Listener};
use js_sys::{Array, Function, Reflect};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{
    HtmlElement, ResizeObserver, ResizeObserverBoxOptions, ResizeObserverEntry,
    ResizeObserverOptions,
};
/// System queries have an independent removable lifetime.
mod media;

/// Safari needs a fresh resolution query after each monitor/zoom change.
mod dpr;

/// One observer plus callbacks and removable media/visibility registrations.
pub(super) struct Monitor {
    /// Disconnecting prevents queued geometry updates after terminal disposal.
    observer: ResizeObserver,
    /// JS calls this trampoline until observer.disconnect completes.
    _callback: Closure<dyn FnMut(Array, ResizeObserver)>,
    /// Explicit listener ownership also covers document and media targets.
    _listeners: Vec<Listener>,
    /// This query handles DPR changes even if CSS container dimensions stay equal.
    _dpr: dpr::DprWatch,
}

/// Every terminal owns one observer; no background polling is used.
impl Monitor {
    /// Watches the actual container and sends initial media state immediately.
    pub(super) fn new<H: DomHost + 'static>(
        state: &Rc<RefCell<KeyboardState<H>>>,
        container: &HtmlElement,
    ) -> Result<Self, JsValue> {
        let weak = Rc::downgrade(state);
        let callback = Closure::wrap(Box::new(move |entries: Array, _: ResizeObserver| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            let Ok(mut state) = state.try_borrow_mut() else {
                return;
            };
            for entry in entries
                .iter()
                .filter_map(|value| value.dyn_into::<ResizeObserverEntry>().ok())
            {
                let dpr = web_sys::window()
                    .map(|window| window.device_pixel_ratio())
                    .unwrap_or(1.0);
                state.host.resize(device_size(&entry, dpr), dpr);
            }
        }) as Box<dyn FnMut(Array, ResizeObserver)>);
        let observer = ResizeObserver::new(callback.as_ref().unchecked_ref())?;
        // Keep the trampoline retained before installing asynchronous observation.
        observe(&observer, container)?;
        let listeners = media::install(state)?;
        let dpr = dpr::DprWatch::new(state, container)?;
        Ok(Self {
            observer,
            _callback: callback,
            _listeners: listeners,
            _dpr: dpr,
        })
    }
}

/// Disconnect before the retained Closure is freed by Rust's field destructors.
impl Drop for Monitor {
    /// Media listeners then detach through their own RAII owners.
    fn drop(&mut self) {
        self.observer.disconnect();
    }
}

/// Uses a fallible reflected call because old Safari can reject the box option.
fn observe(observer: &ResizeObserver, target: &HtmlElement) -> Result<(), JsValue> {
    let options = ResizeObserverOptions::new();
    options.set_box(ResizeObserverBoxOptions::DevicePixelContentBox);
    let function: Function = Reflect::get(observer.as_ref(), &"observe".into())?.dyn_into()?;
    // A failed physical-box registration must not disable automatic fitting.
    if function
        .call2(observer.as_ref(), target.as_ref(), options.as_ref())
        .is_err()
    {
        // Unsupported box sizing is recoverable with the ordinary content box.
        function.call1(observer.as_ref(), target.as_ref())?;
    }
    Ok(())
}

/// Native content-box dimensions are physical pixels, not CSS pixels.
fn device_size(entry: &ResizeObserverEntry, dpr: f64) -> [u32; 2] {
    // Safari may omit this property entirely; a typed Array getter would then
    // yield undefined. Validate the JS array before indexing its first box.
    let boxes = Reflect::get(entry.as_ref(), &"devicePixelContentBoxSize".into())
        .ok()
        .and_then(|value| value.dyn_into::<Array>().ok());
    let first = boxes
        .map(|boxes| boxes.get(0))
        .unwrap_or(JsValue::UNDEFINED);
    let inline = Reflect::get(&first, &"inlineSize".into())
        .ok()
        .and_then(|n| n.as_f64());
    let block = Reflect::get(&first, &"blockSize".into())
        .ok()
        .and_then(|n| n.as_f64());
    if let (Some(width), Some(height)) = (inline, block) {
        return [width.round() as u32, height.round() as u32];
    }
    let rect = entry.content_rect();
    // CSS fallback is rounded only after multiplication, preserving fractional DPR.
    [
        (rect.width() * dpr).round() as u32,
        (rect.height() * dpr).round() as u32,
    ]
}
