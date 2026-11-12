//! Pointer coordinates and browser-default ownership (§12.2, xterm ctlseqs).
//! Compatibility mousedown belongs to the same physical press as pointerdown;
//! its UIEvent.detail preserves the OS double/triple-click policy without a timer.
//!
//! CSS coordinates become device pixels before subtracting grid padding.
//! Signed drag positions reach Session so leaving either grid edge can drive
//! auto-scroll; protocol coordinates separately clamp to valid cells/pixels.
//! The same neutral pointer owner handles drag, word, line, block and extension.
//!
//! Tracking claims application reports except for configured Shift selection.
//! Wheel ownership additionally depends on local history boundaries and mode 1007.
//! Page scrolling is prevented only when the shared routing predicate accepts it.
//! An empty history or a gesture past its boundary keeps the page default.
//! The predicate allocates no wire bytes and cannot advance the fractional ledger.
//! The subsequent typed command repeats policy after interceptors have run.
//!
//! Pointer capture pairs releases outside the canvas for both reports and selection.
//! Release carries its final position, replacing any older coalesced motion sample.
//! Presses and releases go to Session immediately; moves wait for one refresh.
//! Policy is sampled again at delivery, so a live tracking change cancels a drag.
//! Context-menu ownership is distinct: a local right click retains the browser menu.
//! Its preceding mousedown already reported a requested application right click.
//!
//! Touch focuses the hidden textarea and owns a single captured finger.
//! It scrolls history directly, never clicks an ncurses UI or creates a selection.
//! Its finite momentum tail shares the input refresh lane and stops at boundaries.
//! Reduced motion suppresses continuation without disabling direct finger dragging.
//! Cancellation, a later press and disposal release that momentum owner.
//! A second finger cannot replace the active ID in the neutral touch model.
//!
//! Mode 1016 reports device pixels relative to the grid; SGR 1006 reports cells.
//! Both forms originate from the same padding-adjusted hit, without applying DPR twice.
//! Wheel deltaMode zero carries OS pixel inertia rather than a synthetic notch tween.
//! Lines and pages retain row units until Session applies the configured multiplier.
//! Horizontal wheel steps remain button 66/67 reports when an application tracks.
//! They never turn into vertical history motion or alternate-screen arrow keys.
//! X10 requests presses, 1000 also releases, 1002 held motion and 1003 all motion.
//! Browser ownership mirrors those filters while Session remains the encoder owner.
use super::{DomHost, KeyboardState, listener::Listener};
use puzzle_term::{
    messages::Command,
    vt::input::{MouseEvent, MouseKind, MouseTracking},
};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    Event, HtmlCanvasElement, HtmlTextAreaElement, MouseEvent as DomMouse, PointerEvent, WheelEvent,
};

/// CSS-to-device mapping shared by every pointer report.
pub(super) mod geometry;
/// High-rate motion is delivered once per display tick, never by an idle loop.
pub(crate) mod motion;

/// Pointer capture keeps releases paired even when a drag leaves the canvas.
pub(super) fn install<H: DomHost + 'static>(
    state: &Rc<RefCell<KeyboardState<H>>>,
    canvas: &HtmlCanvasElement,
    area: &HtmlTextAreaElement,
    motion: &Rc<motion::Motion<H>>,
) -> Result<Vec<Listener>, JsValue> {
    let mut listeners = Vec::new();
    // Register on the canvas alone: a sibling editor must retain its own gestures.
    for kind in [
        "pointerdown",
        "mousedown",
        "pointerup",
        "pointermove",
        "pointerleave",
        "pointercancel",
        "wheel",
        "contextmenu",
    ] {
        let weak = Rc::downgrade(state);
        let motion = Rc::downgrade(motion);
        // Element handles survive only as long as the removable registration.
        let canvas = canvas.clone();
        let area = area.clone();
        let target = canvas.clone();
        listeners.push(Listener::new(target.as_ref(), kind, move |event| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            let Ok(mut state) = state.try_borrow_mut() else {
                // A synchronously reentered host cannot borrow its Session twice.
                return;
            };
            if let Some(motion) = motion.upgrade() {
                route(&mut state, &canvas, &area, &event, &motion);
            }
        })?);
    }
    Ok(listeners)
}

/// Canvas callbacks do input work only; a host never renders from this lane.
/// Press/release bypass the coalescer so dragging starts and ends promptly.
/// Pending movement is canceled before those transitions to keep FIFO semantics.
/// Wheel reports retain their immediate path alongside keys and paste.
/// Mode changes are sampled on both arrival and deferred motion delivery.
fn route<H: DomHost + 'static>(
    state: &mut KeyboardState<H>,
    canvas: &HtmlCanvasElement,
    area: &HtmlTextAreaElement,
    event: &Event,
    motion: &motion::Motion<H>,
) {
    if event.type_() == "pointerleave" {
        // Cancel a queued inside-hover before ending the canvas hover lifetime.
        motion.cancel();
        state.host.pointer_left();
        return;
    }
    let Some(mouse) = event.dyn_ref::<DomMouse>() else {
        return;
    };
    let geometry = state.host.geometry();
    // The DOM rectangle is CSS-relative and already reflects page scrolling.
    let rect = canvas.get_bounding_client_rect();
    let position = [
        // Convert both axes before hit testing; padding is in device pixels too.
        (f64::from(mouse.client_x()) - rect.left()) * geometry.dpr,
        (f64::from(mouse.client_y()) - rect.top()) * geometry.dpr,
    ];
    let mapped = geometry::map(mouse, geometry, position);
    // Focus/capture can run now, while selection waits for compatibility input.
    // Deferral must leave pointerdown uncanceled or Chrome omits mousedown.
    if defer_press(state, canvas, area, event, mapped, position, motion) {
        return;
    }
    if let Some(wheel) = event.dyn_ref::<WheelEvent>() {
        wheel_input(state, event, wheel, geometry, mapped);
        return;
    }
    let modes = state.host.mouse_modes();
    // Re-query after every event: an app can change tracking between presses.
    let consumed = consumes(mapped, modes.tracking, state.host.shift_selects());
    if event.type_() == "contextmenu" {
        // The paired mousedown already sent the right click; do not send it twice.
        if consumed {
            event.prevent_default();
        }
        return;
    }
    deliver(
        state,
        event,
        mapped,
        (position, geometry),
        (modes.tracking, consumed),
        motion,
    );
}

/// Pairs pointer capture with the same press's native compatibility click count.
/// Returns true for deferred or unpaired input which must not reach Session yet.
fn defer_press<H: DomHost + 'static>(
    state: &mut KeyboardState<H>,
    canvas: &HtmlCanvasElement,
    area: &HtmlTextAreaElement,
    event: &Event,
    mapped: MouseEvent,
    position: [f64; 2],
    motion: &motion::Motion<H>,
) -> bool {
    // Mouse-event inheritance includes wheel and context menu as well as pointer.
    if let Some(pointer) = event.dyn_ref::<PointerEvent>() {
        capture(canvas, area, pointer);
        if pointer.pointer_type() == "touch" {
            motion.touch(state, pointer, position[1]);
            // Basic touch never clicks an application UI or starts text selection.
            event.prevent_default();
            state.pending_mouse_press = None;
            return true;
        }
        if event.type_() == "pointerdown" {
            // Chrome pointerdown.detail is zero. Canceling this event suppresses
            // its compatibility mousedown, which carries the OS click count.
            // Capture/focus now, but start the neutral press only on that mousedown.
            motion.cancel();
            state.pending_mouse_press = Some(mapped.button);
            return true;
        }
        if matches!(event.type_().as_str(), "pointerup" | "pointercancel") {
            state.pending_mouse_press = None;
        }
    }
    // Consume the pending button exactly once; another mousedown cannot
    // repeat the selection start or manufacture a higher native click count.
    if event.type_() == "mousedown" && state.pending_mouse_press.take() != Some(mapped.button) {
        // Another element's/synthetic unpaired mouse event cannot upgrade an old press.
        return true;
    }
    false
}

/// Mirrors the neutral encoder's tracking filters without allocating wire bytes.
fn consumes(event: MouseEvent, tracking: MouseTracking, shift_selects: bool) -> bool {
    use puzzle_term::vt::input::Modifiers;
    if tracking == MouseTracking::None || (shift_selects && event.modifiers.has(Modifiers::SHIFT)) {
        // Local Shift gestures must remain selectable even in an ncurses program.
        return false;
    }
    match event.kind {
        // Plain movement is meaningful only under DECSET 1003; 1002 needs a drag.
        MouseKind::Move => {
            tracking == MouseTracking::AnyMotion
                || (tracking == MouseTracking::ButtonMotion && event.button_held)
        }
        MouseKind::Release => tracking != MouseTracking::X10,
        // A press or wheel is requested by each active tracking mode.
        MouseKind::Press | MouseKind::Wheel => true,
    }
}

/// Capturing the press preserves a matching release outside the canvas.
fn capture(canvas: &HtmlCanvasElement, area: &HtmlTextAreaElement, pointer: &PointerEvent) {
    if pointer.type_() == "pointerdown" {
        // Focus is useful even without tracking; tapping should open a soft keyboard.
        let _ = area.focus();
        // Synthetic events have no active pointer. Rejection must not eat the press.
        let _ = canvas.set_pointer_capture(pointer.pointer_id());
    } else if matches!(pointer.type_().as_str(), "pointerup" | "pointercancel") {
        // Release the browser's drag ownership after the terminal sees its end.
        let _ = canvas.release_pointer_capture(pointer.pointer_id());
    }
}

/// High-rate selection motion shares the report coalescer; release has its final hit.
/// Browser defaults and application ownership are independent of signed drag geometry.
fn deliver<H: DomHost>(
    state: &mut KeyboardState<H>,
    event: &Event,
    mapped: MouseEvent,
    geometry: ([f64; 2], super::Geometry),
    ownership: (MouseTracking, bool),
    motion: &motion::Motion<H>,
) {
    let (position, geometry) = geometry;
    let (tracking, consumed) = ownership;
    let local = tracking == MouseTracking::None
        || (state.host.shift_selects()
            && mapped
                .modifiers
                .has(puzzle_term::vt::input::Modifiers::SHIFT));
    // Left drags belong to selection; hover still reaches core for ownership changes.
    let owned = consumed || (local && mapped.button == 0);
    if owned {
        event.prevent_default();
    }
    let input = puzzle_term::pointer::PointerInput {
        event: mapped,
        position: [
            position[0] - geometry.padding[0],
            position[1] - geometry.padding[1],
        ],
        cell: [geometry.cell[0] as u32, geometry.cell[1] as u32],
    };
    if mapped.kind == MouseKind::Move {
        if motion.queue(input).is_err() {
            tracing::warn!(event = "web.motion_schedule_failed", "Motion rAF rejected");
        }
    } else {
        // Release itself contains the final hit, so older coalesced samples are obsolete.
        motion.cancel();
        if state.host.pointer(input) {
            // A handled region cancels native defaults as well as terminal selection/reporting.
            event.prevent_default();
        }
    }
}

/// Wheel hardware units remain typed until the shared routing/animation owner.
fn wheel_input<H: DomHost>(
    state: &mut KeyboardState<H>,
    event: &Event,
    wheel: &WheelEvent,
    geometry: super::Geometry,
    mut mapped: MouseEvent,
) {
    // Geometry mapping supplies the same cell and modifier sample as pointer input.
    // Only the wheel axis/unit changes; no synthetic key or mouse press is sent.
    mapped.kind = MouseKind::Wheel;
    // Prefer the vertical axis of diagonal gestures; sideways-only input retains
    // M7's xterm 66/67 reports without pretending history has a horizontal viewport.
    let amount = if wheel.delta_y() != 0.0 {
        wheel.delta_y()
    } else {
        mapped.button = if wheel.delta_x() < 0.0 { 66 } else { 67 };
        wheel.delta_x()
    };
    // DOM positive deltas move toward newer text or to the right in the protocol.
    let delta = match wheel.delta_mode() {
        0 => puzzle_term::viewport::ScrollInput::Pixels(amount * geometry.dpr),
        1 => puzzle_term::viewport::ScrollInput::Rows(amount),
        _ => puzzle_term::viewport::ScrollInput::Rows(
            amount * f64::from(geometry.grid[1].saturating_sub(1)),
        ),
    };
    // Cancel the page default only after the live Session accepts this direction.
    if state.host.wheel_consumable(delta, mapped) {
        event.prevent_default();
        state.host.command(Command::Wheel {
            delta,
            pointer: mapped,
        });
    }
}
