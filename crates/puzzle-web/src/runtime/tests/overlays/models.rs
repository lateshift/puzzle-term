//! Real DOM model-region input protects tracking, selection and click-to-follow.
use super::super::{
    viewport::{pointer, rendered, write},
    *,
};

/// Both tracking and local-selection modes use the same built-in hit ownership.
#[wasm_bindgen_test(async)]
async fn scrollbar_and_pill_dom_clicks_consume_tracking_and_selection() {
    for tracking in [false, true] {
        model_case(tracking).await;
    }
}

/// Output and viewport operations are public; coordinates come from the actual M8 track.
async fn model_case(tracking: bool) {
    let (terminal, host) = rendered().await;
    write(&terminal, &"history\r\n".repeat(100)).await;
    terminal.scroll_to_top().unwrap();
    write(&terminal, "new\r\n").await;
    if tracking {
        write(&terminal, "\x1b[?1003h\x1b[?1006h").await;
    }
    let _ = presentation::presented_pixels(&terminal).await;
    let writes = Rc::new(RefCell::new(Vec::new()));
    let sink = writes.clone();
    let callback =
        Closure::wrap(
            Box::new(move |a: js_sys::Uint8Array| sink.borrow_mut().extend(a.to_vec()))
                as Box<dyn FnMut(js_sys::Uint8Array)>,
        );
    terminal
        .on_data(callback.as_ref().unchecked_ref::<Function>().clone())
        .unwrap();
    drag(&terminal, &writes, tracking).await;
    terminal.scroll_to_top().unwrap();
    write(&terminal, "another\r\n").await;
    let _ = presentation::presented_pixels(&terminal).await;
    assert!(terminal.owner.borrow().session.viewport().new_output());
    let pill = {
        let r = terminal.owner.borrow();
        let canvas = r.dom.as_ref().unwrap().canvas();
        [
            f64::from(canvas.width()) / 2.0,
            f64::from(canvas.height()) - 24.0 * r.dpr,
        ]
    };
    at(&terminal, "pointerdown", pill);
    at(&terminal, "pointerup", pill);
    assert_eq!(cursor(&terminal), "pointer");
    assert!(terminal.owner.borrow().session.viewport().following());
    assert!(!terminal.owner.borrow().session.viewport().new_output());
    assert_consumed(&terminal, &writes);
    terminal.dispose().unwrap();
    host.remove();
}

/// Reuses the real DOM fixture helper, expressing physical coordinates in grid units.
fn at(terminal: &PuzzleTerminal, kind: &str, point: [f64; 2]) {
    let (col, row) = {
        let r = terminal.owner.borrow();
        let metrics = r.session.cell_metrics().unwrap();
        let padding = (r.settings.render_padding * r.dpr as f32).round() as f64;
        (
            (point[0] - padding) / f64::from(metrics.width_px),
            (point[1] - padding) / f64::from(metrics.height_px),
        )
    };
    pointer(terminal, kind, col, row, 1, 0, false);
}

/// CSS cursor is a host-visible effect of region picking.
fn cursor(terminal: &PuzzleTerminal) -> String {
    terminal
        .owner
        .borrow()
        .dom
        .as_ref()
        .unwrap()
        .canvas()
        .style()
        .get_property_value("cursor")
        .unwrap()
}

/// Application reports and local selection must both be absent after overlay clicks.
fn assert_consumed(terminal: &PuzzleTerminal, writes: &RefCell<Vec<u8>>) {
    assert!(
        writes.borrow().is_empty(),
        "model regions must not report mouse input"
    );
    assert!(
        terminal
            .selection()
            .unwrap()
            .range()
            .unwrap()
            .is_undefined()
    );
}

/// Captured thumb input uses actual model coordinates and the host's coalesced motion lane.
async fn drag(terminal: &PuzzleTerminal, writes: &RefCell<Vec<u8>>, tracking: bool) {
    let (x, y, bottom) = {
        let r = terminal.owner.borrow();
        let [x, _, w, _] = r.session.viewport().scrollbar.track();
        let [y, h] = r.session.viewport().scrollbar.thumb();
        let [_, top, _, height] = r.session.viewport().scrollbar.track();
        (x + w / 2.0, y + h / 2.0, top + height + h / 2.0)
    };
    at(terminal, "pointermove", [x, y]);
    let _ = presentation::presented_pixels(terminal).await;
    assert_eq!(cursor(terminal), "grab");
    assert!(
        !terminal.owner.borrow().session.viewport().following(),
        "hover must preserve scrollback with tracking={tracking}"
    );
    assert_consumed(terminal, writes);
    at(terminal, "pointerdown", [x, y]);
    assert!(writes.borrow().is_empty(), "press reported input");
    // DOM client coordinates round to integer CSS pixels; move beyond the end
    // so captured drag tests the exact M8 clamp, rather than a subpixel near-end.
    at(terminal, "pointermove", [x, bottom]);
    // The host coalesces mouse motion once per submitted frame (P12).
    let _ = presentation::presented_pixels(terminal).await;
    at(terminal, "pointerup", [x, bottom]);
    assert!(
        terminal.owner.borrow().session.viewport().following(),
        "drag to bottom with tracking={tracking}"
    );
    assert_consumed(terminal, writes);
}
