//! Trusted Chrome presses protect the public word/logical-line selection path.
//! WebDriver supplies input to this same test page; JS never dispatches a press.
use super::{
    viewport::{rendered, write},
    *,
};

/// Real pointerdown has detail zero; the same press's mousedown supplies OS count.
#[wasm_bindgen_test(async)]
async fn trusted_double_and_triple_click_select_word_and_wrapped_line() {
    let (terminal, host) = rendered().await;
    write(&terminal, "alpha beta\r\nabcdefghijklmnopqrstUV\r\nlast").await;
    let canvas = terminal
        .owner
        .borrow()
        .dom
        .as_ref()
        .unwrap()
        .canvas()
        .clone();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let sink = observed.clone();
    let listener = Closure::wrap(Box::new(move |event: web_sys::MouseEvent| {
        sink.borrow_mut()
            .push((event.type_(), event.detail(), event.is_trusted()));
    }) as Box<dyn FnMut(web_sys::MouseEvent)>);
    for kind in ["pointerdown", "mousedown"] {
        canvas
            .add_event_listener_with_callback(kind, listener.as_ref().unchecked_ref())
            .unwrap();
    }
    let double_ok = presses(&terminal, 7.5, 0.5, 2).await;
    let selection = terminal.selection().unwrap();
    let word = selection.text().unwrap();
    selection.clear().unwrap();
    // Another location starts a distinct OS press sequence without an invented timer.
    let triple_ok = presses(&terminal, 2.5, 1.5, 3).await;
    let line = selection.text().unwrap();
    for kind in ["pointerdown", "mousedown"] {
        canvas
            .remove_event_listener_with_callback(kind, listener.as_ref().unchecked_ref())
            .unwrap();
    }
    let events = observed.borrow().clone();
    terminal.dispose().unwrap();
    host.remove();
    // Release owners before a failing wasm assertion so later device tests can run.
    wasm_bindgen_test::console_log!(
        "trusted press events: {:?}; word={:?}, line={:?}",
        events,
        word,
        line
    );
    assert!(double_ok && triple_ok, "WebDriver actions must succeed");
    assert_eq!(word, "beta");
    assert_eq!(line, "abcdefghijklmnopqrstUV");
    assert_trusted_counts(&events);
}

/// Counts must come from the actual compatibility mouse event of each press.
fn assert_trusted_counts(events: &[(String, i32, bool)]) {
    assert!(
        events.iter().all(|event| event.2),
        "every observed press must be trusted"
    );
    let pointers: Vec<_> = events
        .iter()
        .filter(|e| e.0 == "pointerdown")
        .map(|e| e.1)
        .collect();
    let mice: Vec<_> = events
        .iter()
        .filter(|e| e.0 == "mousedown")
        .map(|e| e.1)
        .collect();
    assert_eq!(pointers, vec![0; 5]);
    assert_eq!(mice, vec![1, 2, 1, 2, 3]);
}

/// Request only physical pointer actions; the endpoint cannot mutate terminal state.
async fn presses(t: &PuzzleTerminal, col: f64, row: f64, count: u8) -> bool {
    let url = option_env!("PUZZLE_TEST_ACTIONS_URL").unwrap_or("http://127.0.0.1:8094/press");
    let (x, y) = {
        let r = t.owner.borrow();
        let rect = r.dom.as_ref().unwrap().canvas().get_bounding_client_rect();
        let metrics = r.session.cell_metrics().unwrap();
        let padding = (r.settings.render_padding * r.dpr as f32).round() as f64;
        let x = (rect.left() + (padding + col * f64::from(metrics.width_px)) / r.dpr).round();
        let y = (rect.top() + (padding + row * f64::from(metrics.height_px)) / r.dpr).round();
        (x, y)
    };
    // A real HTTP response acknowledges completion of the driver's action sequence.
    let result = JsFuture::from(
        window()
            .unwrap()
            .fetch_with_str(&format!("{url}?x={x}&y={y}&count={count}")),
    )
    .await;
    result
        .ok()
        .and_then(|v| v.dyn_into::<web_sys::Response>().ok())
        .is_some_and(|r| r.ok())
}
