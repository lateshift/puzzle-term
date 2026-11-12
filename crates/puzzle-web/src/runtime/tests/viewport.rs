//! Real served assets and public API exercise browser input and Rust view models.
use super::*;
use puzzle_term::vt::input::Modifiers;

/// A fixed grid makes independently authored cell positions stable at both DPRs.
pub(super) async fn rendered() -> (PuzzleTerminal, web_sys::HtmlElement) {
    let document = window().unwrap().document().unwrap();
    let host = document
        .create_element("div")
        .unwrap()
        .dyn_into::<web_sys::HtmlElement>()
        .unwrap();
    host.style().set_css_text("width:800px;height:480px");
    document.body().unwrap().append_child(&host).unwrap();
    let options = js_sys::JSON::parse(r##"{"cols":20,"rows":6,"settings":{"font":{"ligatures":false},"cursor":{"blink":false},"scrolling":{"smooth":false},"theme":{"mode":"dark","dark":"probe"},"themes":{"probe":{"extends":"puzzle-dark","searchMatch":"#334455","searchCurrent":"#775533","selectionBackground":"#225566"}}}}"##).unwrap();
    let terminal = PuzzleTerminal::create(host.clone(), Some(options))
        .await
        .unwrap();
    let resource = terminal.owner.borrow().text.clone().unwrap();
    JsFuture::from(super::super::resources::load_fonts(&resource))
        .await
        .unwrap();
    // Fit through the public sizing API after font readiness, before any test output.
    terminal.resize(20.into(), 6.into()).unwrap();
    // Asset loading precedes the first refresh; wait for the actual fitted grid.
    let _ = presentation::presented_pixels(&terminal).await;
    assert_eq!(
        (terminal.cols().unwrap(), terminal.rows().unwrap()),
        (20, 6)
    );
    let history = terminal
        .owner
        .borrow()
        .session
        .terminal()
        .screen()
        .ring
        .scrollback_len();
    assert_eq!(
        history, 0,
        "fixed-grid create must not manufacture blank history"
    );
    (terminal, host)
}
/// Writes only through public output; await means all parser work has completed.
pub(super) async fn write(t: &PuzzleTerminal, text: &str) {
    JsFuture::from(t.write(text.into()).unwrap()).await.unwrap();
}
/// Captured coordinates preserve signed outside samples and click counts.
pub(super) fn pointer(
    t: &PuzzleTerminal,
    kind: &str,
    col: f64,
    row: f64,
    count: i32,
    modifiers: u8,
    touch: bool,
) {
    let r = t.owner.borrow();
    let dom = r.dom.as_ref().unwrap();
    let rect = dom.canvas().get_bounding_client_rect();
    let metrics = r.session.cell_metrics().unwrap();
    let padding = (r.settings.render_padding * r.dpr as f32).round() as f64;
    let init = web_sys::PointerEventInit::new();
    init.set_cancelable(true);
    init.set_pointer_id(1);
    init.set_pointer_type(if touch { "touch" } else { "mouse" });
    init.set_client_x(
        (rect.left() + (padding + col * f64::from(metrics.width_px)) / r.dpr).round() as i32,
    );
    init.set_client_y(
        (rect.top() + (padding + row * f64::from(metrics.height_px)) / r.dpr).round() as i32,
    );
    init.set_button(0);
    init.set_buttons(if kind == "pointerup" { 0 } else { 1 });
    // Pointer Events carry no Chrome press count; compatibility mousedown does.
    init.set_detail(0);
    init.set_shift_key(modifiers & Modifiers::SHIFT != 0);
    init.set_alt_key(modifiers & Modifiers::ALT != 0);
    let canvas = dom.canvas().clone();
    drop(r);
    let event = web_sys::PointerEvent::new_with_event_init_dict(kind, &init).unwrap();
    canvas.dispatch_event(&event).unwrap();
    if kind == "pointerdown" && !touch {
        let mouse = web_sys::MouseEventInit::new();
        mouse.set_client_x(event.client_x());
        mouse.set_client_y(event.client_y());
        mouse.set_button(0);
        mouse.set_buttons(1);
        mouse.set_detail(count);
        mouse.set_shift_key(modifiers & Modifiers::SHIFT != 0);
        mouse.set_alt_key(modifiers & Modifiers::ALT != 0);
        mouse.set_cancelable(true);
        canvas
            .dispatch_event(
                &web_sys::MouseEvent::new_with_mouse_event_init_dict("mousedown", &mouse).unwrap(),
            )
            .unwrap();
    }
}
/// Local boundaries, tracking precedence, mode 1007 and all DOM units own defaults.
#[wasm_bindgen_test(async)]
async fn wheel_consumption_and_units_follow_shared_routing() {
    let (t, host) = rendered().await;
    let writes = Rc::new(RefCell::new(Vec::new()));
    let sink = writes.clone();
    let cb =
        Closure::wrap(
            Box::new(move |a: js_sys::Uint8Array| sink.borrow_mut().extend(a.to_vec()))
                as Box<dyn FnMut(js_sys::Uint8Array)>,
        );
    t.on_data(cb.as_ref().unchecked_ref::<Function>().clone())
        .unwrap();
    for tracking in [0, 9, 1000, 1002, 1003] {
        for alternate in [false, true] {
            for alt_scroll in [false, true] {
                for shift in [false, true] {
                    for unit in [0, 1, 2] {
                        routing_case(&t, &writes, tracking, alternate, alt_scroll, shift, unit)
                            .await;
                    }
                }
            }
        }
    }
    write(&t, "\x1bc").await;
    assert!(
        !wheel(&t, -1.0, 1, false),
        "empty primary history passes page scroll"
    );
    write(&t, &"history\r\n".repeat(30)).await;
    assert!(
        !wheel(&t, 1.0, 1, false),
        "live tail passes downward page scroll"
    );
    t.scroll_to_top().unwrap();
    assert!(
        !wheel(&t, -1.0, 1, false),
        "oldest boundary passes upward page scroll"
    );
    assert!(wheel(&t, 1.0, 1, false));
    horizontal_reports(&t, &writes).await;
    t.dispose().unwrap();
    host.remove();
}
/// Each case checks browser ownership and exact application bytes, independently.
async fn routing_case(
    t: &PuzzleTerminal,
    writes: &Rc<RefCell<Vec<u8>>>,
    tracking: u16,
    alternate: bool,
    alt_scroll: bool,
    shift: bool,
    unit: u32,
) {
    write(t, "\x1bc").await;
    write(t, &"history\r\n".repeat(30)).await;
    let modes = format!(
        "{}\x1b[?1007{}\x1b[?1006h{}",
        if alternate { "\x1b[?1049h" } else { "" },
        if alt_scroll { 'h' } else { 'l' },
        if tracking == 0 {
            String::new()
        } else {
            format!("\x1b[?{tracking}h")
        }
    );
    write(t, &modes).await;
    writes.borrow_mut().clear();
    let pixels = {
        let r = t.owner.borrow();
        f64::from(r.session.cell_metrics().unwrap().height_px) / r.dpr
    };
    let consumed = wheel(t, if unit == 0 { -pixels } else { -1.0 }, unit, shift);
    let application = !shift && (tracking != 0 || (alternate && alt_scroll));
    assert_eq!(consumed, application || !alternate);
    let steps = if unit == 2 { 5 } else { 1 };
    if !application {
        assert!(writes.borrow().is_empty());
    } else if tracking != 0 {
        assert_eq!(writes.borrow().as_slice(), b"\x1b[<64;1;1M".repeat(steps));
    } else {
        assert_eq!(writes.borrow().as_slice(), b"\x1b[A".repeat(steps));
    }
}
/// Actual non-passive listeners determine whether the page can scroll.
fn wheel(t: &PuzzleTerminal, delta: f64, mode: u32, shift: bool) -> bool {
    let init = web_sys::WheelEventInit::new();
    init.set_cancelable(true);
    init.set_delta_y(delta);
    init.set_delta_mode(mode);
    init.set_shift_key(shift);
    let canvas = t.owner.borrow().dom.as_ref().unwrap().canvas().clone();
    let event = web_sys::WheelEvent::new_with_event_init_dict("wheel", &init).unwrap();
    canvas.dispatch_event(&event).unwrap();
    event.default_prevented()
}
/// Public scrolling variants and facts retain stable line and prompt identities.
#[wasm_bindgen_test(async)]
async fn viewport_api_members_and_scroll_event() {
    let (t, host) = rendered().await;
    write(&t, &"history\r\n".repeat(30)).await;
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let cb = Closure::wrap(Box::new(move |v: JsValue| {
        sink.borrow_mut()
            .push(value::get(&v, "offsetPx").unwrap().as_f64().unwrap())
    }) as Box<dyn FnMut(JsValue)>);
    let handle = t
        .on(
            "scroll".into(),
            cb.as_ref().unchecked_ref::<Function>().clone(),
        )
        .unwrap();
    t.scroll_lines((-2).into()).unwrap();
    assert_eq!(t.owner.borrow().session.viewport().offset_rows(), 2);
    t.scroll_pages((-1).into()).unwrap();
    assert_eq!(t.owner.borrow().session.viewport().offset_rows(), 7);
    t.scroll_to_top().unwrap();
    let ids: Vec<_> = {
        let r = t.owner.borrow();
        (0..20)
            .map(|i| r.session.terminal().screen().ring.retained(i).unwrap().id.0)
            .collect()
    };
    t.scroll_to_line((ids[10] as f64).into(), Some("top".into()))
        .unwrap();
    assert_eq!(t.owner.borrow().session.viewport().top_index(), 10);
    t.scroll_to_line((ids[10] as f64).into(), None).unwrap();
    assert_eq!(t.owner.borrow().session.viewport().top_index(), 7);
    t.scroll_to_bottom().unwrap();
    write(
        &t,
        "\x1b]133;A\x07prompt\r\n\x1b]133;B\x07cmd\r\n\x1b]133;C\x07out\r\n\x1b]133;D;0\x07",
    )
    .await;
    t.scroll_to_top().unwrap();
    // Retain enough following output that top alignment is not clamped at the tail.
    write(&t, &"after prompt\r\n".repeat(10)).await;
    t.scroll_to_top().unwrap();
    t.scroll_to_prompt(1.into()).unwrap();
    assert_prompt_at_top(&t);
    assert!(seen.borrow().len() >= 7);
    let count = seen.borrow().len();
    handle.dispose();
    t.scroll_to_top().unwrap();
    assert_eq!(seen.borrow().len(), count);
    assert!(t.scroll_lines(f64::NAN.into()).is_err());
    assert!(t.scroll_to_line((-1).into(), None).is_err());
    t.dispose().unwrap();
    host.remove();
}
/// One finger scrolls locally, a second cannot take over, and release momentum ends.
#[wasm_bindgen_test(async)]
async fn touch_drag_momentum_and_tap_focus() {
    let (t, host) = rendered().await;
    let result = touch_gesture(&t).await;
    // A wasm panic does not unwind owners: release GPU state before asserting.
    t.dispose().unwrap();
    host.remove();
    assert!(result.is_ok(), "{}", result.unwrap_err());
}

/// Real DOM timing checks direction and reduced-motion/kinetic ownership.
async fn touch_gesture(t: &PuzzleTerminal) -> Result<(), String> {
    write(&t, &"history\r\n".repeat(80)).await;
    pointer(&t, "pointerdown", 2.5, 1.5, 1, 0, true);
    if !t.has_focus().unwrap() {
        return Err("touch tap did not focus".into());
    }
    // Real elapsed input timestamps permit a fresh velocity estimate.
    socket::delay(10).await;
    pointer(&t, "pointermove", 2.5, 3.5, 1, 0, true);
    let drag = t.owner.borrow().session.viewport().scroll_px();
    if drag <= 0.0 {
        return Err(format!("touch drag did not move: {drag}"));
    }
    pointer(&t, "pointerup", 2.5, 3.5, 1, 0, true);
    // The default fixture reduces motion, so no momentum is allowed.
    socket::delay(40).await;
    if t.owner.borrow().session.viewport().scroll_px() != drag {
        return Err("reduced-motion drag launched momentum".into());
    }
    t.owner.borrow_mut().settings.scrolling_smooth = true;
    pointer(&t, "pointerdown", 2.5, 1.5, 1, 0, true);
    socket::delay(10).await;
    pointer(&t, "pointermove", 2.5, 3.5, 1, 0, true);
    let released = t.owner.borrow().session.viewport().scroll_px();
    pointer(&t, "pointerup", 2.5, 3.5, 1, 0, true);
    socket::delay(80).await;
    let after = t.owner.borrow().session.viewport().scroll_px();
    if after <= released {
        return Err(format!("momentum did not continue: {released} -> {after}"));
    }
    Ok(())
}

/// Prompt navigation aligns the retained parsed mark rather than a numeric row guess.
fn assert_prompt_at_top(t: &PuzzleTerminal) {
    let prompt = t
        .owner
        .borrow()
        .session
        .buffer()
        .zones()
        .find(|m| m.kind == puzzle_term::vt::zones::ZoneKind::Prompt)
        .unwrap()
        .line;
    assert_eq!(
        t.owner.borrow().session.buffer().viewport().unwrap().first,
        prompt
    );
}

/// M7 sideways wheel reports keep their axis and never scroll vertical history.
async fn horizontal_reports(t: &PuzzleTerminal, writes: &Rc<RefCell<Vec<u8>>>) {
    write(t, "\x1bc\x1b[?1000;1006h").await;
    let canvas = t.owner.borrow().dom.as_ref().unwrap().canvas().clone();
    for (delta, button) in [(-1.0, 66), (1.0, 67)] {
        writes.borrow_mut().clear();
        let init = web_sys::WheelEventInit::new();
        init.set_cancelable(true);
        init.set_delta_mode(1);
        init.set_delta_x(delta);
        let event = web_sys::WheelEvent::new_with_event_init_dict("wheel", &init).unwrap();
        canvas.dispatch_event(&event).unwrap();
        assert!(event.default_prevented());
        assert_eq!(
            writes.borrow().as_slice(),
            format!("\x1b[<{button};1;1M").as_bytes()
        );
        // Shift has no sideways local destination, so the embedding page keeps it.
        init.set_shift_key(true);
        let event = web_sys::WheelEvent::new_with_event_init_dict("wheel", &init).unwrap();
        canvas.dispatch_event(&event).unwrap();
        assert!(!event.default_prevented());
    }
}
