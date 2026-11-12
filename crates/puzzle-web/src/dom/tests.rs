//! Real browser event dispatch without a GPU or a user's OS input method.
#![allow(
    clippy::unwrap_used,
    reason = "browser setup and assertions are test-only"
)]
/// Recording host keeps commands visible without duplicating Session semantics.
mod support;
use super::*;
use puzzle_term::{messages::Command, vt::input::MouseTracking};
use support::*;
use wasm_bindgen_test::wasm_bindgen_test;
use web_sys::{
    ClipboardEvent, ClipboardEventInit, CompositionEvent, CompositionEventInit, DataTransfer,
    InputEvent, InputEventInit, PointerEvent, PointerEventInit, WheelEvent, WheelEventInit,
};

/// A Japanese candidate updates UI only and commits once despite a trailing input.
#[wasm_bindgen_test]
fn mounted_composition_and_disposal() {
    let fixture = Fixture::new();
    let area = fixture.input.textarea();
    for (kind, text) in [("compositionstart", ""), ("compositionupdate", "日本")] {
        let init = CompositionEventInit::new();
        init.set_data(text);
        area.dispatch_event(&CompositionEvent::new_with_event_init_dict(kind, &init).unwrap())
            .unwrap();
    }
    assert!(
        !fixture
            .commands
            .borrow()
            .iter()
            .any(|c| matches!(c, Command::Text(_)))
    );
    let end = CompositionEventInit::new();
    end.set_data("日本");
    area.dispatch_event(
        &CompositionEvent::new_with_event_init_dict("compositionend", &end).unwrap(),
    )
    .unwrap();
    let echo = InputEventInit::new();
    echo.set_data(Some("日本"));
    area.dispatch_event(&InputEvent::new_with_event_init_dict("input", &echo).unwrap())
        .unwrap();
    assert_eq!(
        fixture
            .commands
            .borrow()
            .iter()
            .filter(|c| matches!(c, Command::Text(t) if t == "日本"))
            .count(),
        1
    );
    assert!(area.value().is_empty());
    let retained = area.clone();
    let commands = fixture.commands.clone();
    drop(fixture);
    let count = commands.borrow().len();
    // Retaining a detached element cannot keep its old Session listener alive.
    retained
        .dispatch_event(&InputEvent::new_with_event_init_dict("input", &echo).unwrap())
        .unwrap();
    assert_eq!(commands.borrow().len(), count);
}

/// Clipboard data is owned once; no permission request or duplicate input follows.
#[wasm_bindgen_test]
fn mounted_paste_from_event() {
    let fixture = Fixture::new();
    let data = DataTransfer::new().unwrap();
    data.set_data("text/plain", "paste\n日本").unwrap();
    let init = ClipboardEventInit::new();
    init.set_clipboard_data(Some(&data));
    init.set_cancelable(true);
    let paste = ClipboardEvent::new_with_event_init_dict("paste", &init).unwrap();
    assert!(!fixture.input.textarea().dispatch_event(&paste).unwrap());
    assert!(paste.default_prevented());
    assert!(matches!(&fixture.commands.borrow()[0], Command::Paste(t) if t == "paste\n日本"));
}

/// The non-passive listener cancels page scrolling only for a requested report.
#[wasm_bindgen_test]
fn mounted_wheel_default_ownership() {
    let fixture = Fixture::new();
    for (tracking, shift, consumed) in [
        (MouseTracking::None, false, false),
        (MouseTracking::Normal, false, true),
        (MouseTracking::Normal, true, false),
    ] {
        fixture.tracking.set(tracking);
        let init = WheelEventInit::new();
        init.set_delta_y(-40.0);
        init.set_shift_key(shift);
        init.set_cancelable(true);
        let wheel = WheelEvent::new_with_event_init_dict("wheel", &init).unwrap();
        fixture.input.canvas().dispatch_event(&wheel).unwrap();
        assert_eq!(wheel.default_prevented(), consumed);
    }
    let commands = fixture.commands.borrow();
    assert_eq!(commands.len(), 1);
    assert!(
        matches!(commands[0], Command::Wheel { delta: puzzle_term::viewport::ScrollInput::Pixels(n), .. } if n < 0.0)
    );
}

/// DOM button bits, click counts and DPR mapping reach neutral pointer metadata.
#[wasm_bindgen_test]
fn pointer_device_geometry() {
    let init = PointerEventInit::new();
    init.set_buttons(4);
    init.set_button(-1);
    // Real Chrome pointer motion carries no click count.
    init.set_detail(0);
    init.set_ctrl_key(true);
    let event = PointerEvent::new_with_event_init_dict("pointermove", &init).unwrap();
    let geometry = geometry();
    let mapped = pointer::geometry::map(event.as_ref(), geometry, [44.0, 84.0]);
    assert_eq!((mapped.column, mapped.row), (2, 2));
    assert_eq!((mapped.pixel_x, mapped.pixel_y), (40, 80));
    assert_eq!(
        (mapped.button, mapped.click_count, mapped.modifiers.0),
        (1, 0, 4)
    );
}

/// High-rate movement writes once on rAF; a disposed owner cancels its turn.
#[wasm_bindgen_test]
async fn pointer_motion_is_coalesced() {
    let fixture = Fixture::new();
    fixture.tracking.set(MouseTracking::AnyMotion);
    for x in [20, 40, 60] {
        let init = PointerEventInit::new();
        init.set_client_x(x);
        init.set_client_y(20);
        fixture
            .input
            .canvas()
            .dispatch_event(&PointerEvent::new_with_event_init_dict("pointermove", &init).unwrap())
            .unwrap();
    }
    assert!(fixture.commands.borrow().is_empty());
    next_refresh().await;
    assert_eq!(fixture.commands.borrow().len(), 1);
    assert!(matches!(fixture.commands.borrow()[0], Command::Pointer(p) if p.event.pixel_x == 116));
    fixture.commands.borrow_mut().clear();
    let init = PointerEventInit::new();
    fixture
        .input
        .canvas()
        .dispatch_event(&PointerEvent::new_with_event_init_dict("pointermove", &init).unwrap())
        .unwrap();
    let commands = fixture.commands.clone();
    drop(fixture);
    next_refresh().await;
    assert!(commands.borrow().is_empty());
}

/// Resolves on an actual display callback after the adapter's scheduled motion.
async fn next_refresh() {
    use wasm_bindgen::{JsCast, closure::Closure};
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let callback = Closure::once_into_js(move |_: f64| {
            let _ = resolve.call0(&wasm_bindgen::JsValue::UNDEFINED);
        });
        web_sys::window()
            .unwrap()
            .request_animation_frame(callback.unchecked_ref())
            .unwrap();
    });
    wasm_bindgen_futures::JsFuture::from(promise).await.unwrap();
}
