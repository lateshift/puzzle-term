//! Pointer coalescing and finite touch momentum share one input refresh lane (P12).
//!
//! Motion can arrive far faster than display refresh. Keeping only the latest
//! copied event avoids flooding a remote program with positions never displayed.
//! Presses, releases, keys and paste still take their immediate ordered path.
//! This is an input coalescer, not a renderer; it never builds or paints a frame.
//!
//! There is no continuously running rAF loop. A requested motion schedules one
//! turn; that turn clears the request ID before delivering the latest snapshot.
//! Another event can then schedule the next tick. Idle terminals request none.
//! Canceled canvas drags likewise remove their pending motion on disposal.
//!
//! The callback's owner and Session state are both weak. Keeping a pending
//! display callback cannot retain a terminal removed from the page.
//! A retained event contains only copied neutral metadata, never a DOM event
//! or a reference into wasm memory that could expire before the next refresh.
//!
//! Tracking is queried again at delivery because application output may have
//! changed modes while the report waited for rAF. Disabling tracking must not
//! let a previously queued move become bytes after the disable took effect.
use crate::dom::{DomHost, KeyboardState};
use puzzle_term::{messages::Command, pointer::PointerInput, touch::Touch};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};

/// One lazy rAF request and its most recent pointer snapshot.
pub(crate) struct Motion<H: DomHost> {
    /// Overwriting pending motion cannot reorder presses or application bytes.
    pending: Cell<Option<PointerInput>>,
    /// One finger and its finite momentum tail share this lazy input tick.
    touch: Cell<Touch>,
    /// No second request is issued until the first display callback runs.
    frame: Cell<Option<i32>>,
    /// Retained trampoline; its weak self-reference cannot keep this owner alive.
    callback: Closure<dyn FnMut(f64)>,
    /// A removed terminal must not be resurrected by a queued callback.
    state: Weak<RefCell<KeyboardState<H>>>,
}

/// Input remains immediate except for explicitly coalesced high-rate motion.
impl<H: DomHost> Motion<H> {
    /// A weak cycle lets the callback find state without leaking registrations.
    pub(crate) fn new(state: &Rc<RefCell<KeyboardState<H>>>) -> Rc<Self>
    where
        H: 'static,
    {
        Rc::new_cyclic(|weak: &Weak<Self>| {
            let owner = weak.clone();
            let callback = Closure::wrap(Box::new(move |time: f64| {
                if let Some(owner) = owner.upgrade() {
                    owner.flush(time);
                }
            }) as Box<dyn FnMut(f64)>);
            Self {
                pending: Cell::new(None),
                touch: Cell::new(Touch::default()),
                frame: Cell::new(None),
                callback,
                state: Rc::downgrade(state),
            }
        })
    }

    /// Schedules work only when a requested motion report actually arrives.
    pub(crate) fn queue(&self, event: PointerInput) -> Result<(), JsValue> {
        // Only the most recent position is observable at the next display tick.
        self.pending.set(Some(event));
        self.schedule()
    }
    /// Reuses one callback for pending pointer input or an active momentum tail.
    fn schedule(&self) -> Result<(), JsValue> {
        if self.frame.get().is_none() {
            let window =
                web_sys::window().ok_or_else(|| JsValue::from_str("Missing browser window"))?;
            self.frame.set(Some(
                window.request_animation_frame(self.callback.as_ref().unchecked_ref())?,
            ));
        }
        Ok(())
    }
    /// Touch samples never become application mouse clicks; Shift marks local routing.
    pub(crate) fn touch(
        &self,
        state: &mut KeyboardState<H>,
        event: &web_sys::PointerEvent,
        y: f64,
    ) {
        // Copy numerical gesture state out of Cell; host command dispatch may reenter
        // browser callbacks and must never borrow a RefCell containing this gesture.
        let mut touch = self.touch.get();
        // PointerEvent timestamps and rAF timestamps share the DOM monotonic epoch.
        let at = event.time_stamp();
        match event.type_().as_str() {
            "pointerdown" => {
                // The neutral owner rejects another finger until this ID releases.
                touch.press(event.pointer_id(), y, at);
            }
            "pointermove" => {
                // Drag displacement is immediate; only its eventual momentum needs rAF.
                if let Some(pixels) = touch.moved(event.pointer_id(), y, at) {
                    local_scroll(state, pixels);
                }
            }
            "pointerup" => touch.release(event.pointer_id(), at, state.host.motion_reduced()),
            // Browser cancellation releases this ID with momentum explicitly suppressed.
            "pointercancel" => touch.release(event.pointer_id(), at, true),
            _ => {}
        }
        // No tick is registered for a tap or a reduced-motion release.
        self.touch.set(touch);
        if touch.animating() && self.schedule().is_err() {
            self.touch.set(Touch::default());
            tracing::warn!(event = "web.motion_schedule_failed", "Touch rAF rejected");
        }
    }

    /// Cancels the browser callback before its Closure can be released.
    pub(crate) fn cancel(&self) {
        if let Some(id) = self.frame.take()
            && let Some(window) = web_sys::window()
        {
            let _ = window.cancel_animation_frame(id);
        }
        self.pending.set(None);
        self.touch.set(Touch::default());
    }

    /// Dispatches the latest pointer through live policy, then advances local inertia.
    fn flush(&self, now: f64) {
        // Clear the request before dispatch, permitting a reentered event to reschedule.
        self.frame.set(None);
        let pending = self.pending.take();
        if let Some(state) = self.state.upgrade()
            && let Ok(mut state) = state.try_borrow_mut()
        {
            if let Some(input) = pending {
                // Session rechecks tracking/Shift, including canceling a stale local drag.
                state.host.pointer(input);
            }
            // Momentum is separate from selection motion and cannot synthesize a press.
            let mut touch = self.touch.get();
            // A media/settings change ends an already launched tail at its next tick.
            if state.host.motion_reduced() {
                touch.cancel();
            }
            // A clamped boundary must stop the tail; otherwise it would poll uselessly.
            if let Some(pixels) = touch.advance(now)
                && !local_scroll(&mut state, pixels)
            {
                touch.cancel();
            }
            // Store the decayed velocity before requesting another input refresh.
            self.touch.set(touch);
            // Scheduling failure relinquishes ownership rather than retaining idle inertia.
            if touch.animating() && self.schedule().is_err() {
                self.touch.set(Touch::default());
            }
        }
    }
}

/// A removed canvas cannot retain an animation callback holding its host.
impl<H: DomHost> Drop for Motion<H> {
    /// Releases the display request before the retained trampoline is freed.
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Touch is local history even when a full-screen application tracks mouse input.
fn local_scroll<H: DomHost>(state: &mut KeyboardState<H>, pixels: f64) -> bool {
    use puzzle_term::{
        viewport::ScrollInput,
        vt::input::{Modifiers, MouseEvent, MouseKind},
    };
    // Shift is a routing override for touch, independent of mouse.shiftSelects.
    // Position zero is irrelevant because this request can never reach reporting.
    let pointer = MouseEvent {
        kind: MouseKind::Wheel,
        button: 65,
        column: 0,
        row: 0,
        pixel_x: 0,
        pixel_y: 0,
        modifiers: Modifiers(Modifiers::SHIFT),
        button_held: false,
        click_count: 0,
    };
    // Device deltas preserve direct drag speed; viewport applies the user multiplier.
    let delta = ScrollInput::Pixels(pixels);
    // The local override cannot leak a fabricated Shift modifier into protocol reports.
    let consumed = state.host.wheel_consumable(delta, pointer);
    // Consumption is checked before dispatch so inertia can stop at either edge.
    if consumed {
        state.host.command(Command::Wheel { delta, pointer });
    }
    consumed
}
