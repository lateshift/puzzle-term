//! Actual DOM clicks and hover must reach shared overlay regions before VT (§11.4).
//! Rust installs the M9 declaration; the JS decorate facade deliberately remains M10.
/// Model regions use the same actual DOM routing as declarative annotations.
mod models;
use super::{
    viewport::{pointer, rendered, write},
    *,
};
use puzzle_term::{
    frame::ZSlot,
    overlay::{
        anchor::{Anchor, MarkerRange},
        decoration::*,
        geometry::Cell,
    },
};
/// Both local selection and tracking modes must give an annotation the same ownership.
#[wasm_bindgen_test(async)]
async fn overlay_regions_consume_dom_clicks_and_set_hover_cursor() {
    for tracking in [false, true] {
        dom_case(tracking).await;
    }
}
/// Real served fonts, submitted frames, DOM events and the ordinary onData transport.
async fn dom_case(tracking: bool) {
    let (terminal, host) = rendered().await;
    write(
        &terminal,
        if tracking {
            "\x1b[?1003h\x1b[?1006halpha beta"
        } else {
            "alpha beta"
        },
    )
    .await;
    let writes = Rc::new(RefCell::new(Vec::new()));
    let sink = writes.clone();
    let callback = Closure::wrap(Box::new(move |bytes: js_sys::Uint8Array| {
        sink.borrow_mut().extend(bytes.to_vec());
    }) as Box<dyn FnMut(js_sys::Uint8Array)>);
    terminal
        .on_data(callback.as_ref().unchecked_ref::<Function>().clone())
        .unwrap();
    install_annotation(&terminal);
    let _ = presentation::presented_pixels(&terminal).await;
    pointer(&terminal, "pointerdown", 1.5, 0.5, 1, 0, false);
    pointer(&terminal, "pointerup", 3.5, 0.5, 1, 0, false);
    let canvas = assert_consumed(&terminal, &writes);
    let pixels = presentation::presented_pixels(&terminal).await;
    assert!(
        !pixels.is_empty(),
        "tooltip frame reaches the real browser canvas"
    );
    pointer(&terminal, "pointerleave", 3.5, 0.5, 1, 0, false);
    assert_eq!(canvas.style().get_property_value("cursor").unwrap(), "text");
    let closed = presentation::presented_pixels(&terminal).await;
    assert_ne!(
        pixels, closed,
        "tooltip paints visible ink and disappears on leave"
    );
    // An outside click proves that the annotation did not globally disable terminal input.
    pointer(&terminal, "pointerdown", 7.5, 0.5, 1, 0, false);
    pointer(&terminal, "pointerup", 8.5, 0.5, 1, 0, false);
    if tracking {
        assert!(!writes.borrow().is_empty());
    } else {
        assert!(
            !terminal
                .selection()
                .unwrap()
                .range()
                .unwrap()
                .is_undefined()
        );
    }
    terminal.dispose().unwrap();
    host.remove();
}

/// Selection/reporting suppression and the canvas cursor are observable host behavior.
fn assert_consumed(
    terminal: &PuzzleTerminal,
    writes: &RefCell<Vec<u8>>,
) -> web_sys::HtmlCanvasElement {
    assert!(
        terminal
            .selection()
            .unwrap()
            .range()
            .unwrap()
            .is_undefined()
    );
    assert!(
        writes.borrow().is_empty(),
        "annotation clicks never produce mouse reports"
    );
    let canvas = terminal
        .owner
        .borrow()
        .dom
        .as_ref()
        .unwrap()
        .canvas()
        .clone();
    assert_eq!(
        canvas.style().get_property_value("cursor").unwrap(),
        "pointer"
    );
    canvas
}

/// The Rust side exists in M9; no M10 JavaScript decoration facade is required.
fn install_annotation(terminal: &PuzzleTerminal) {
    {
        let mut runtime = terminal.owner.borrow_mut();
        let session = &mut runtime.session;
        let line = session.buffer().cursor().unwrap().line;
        let start = session.add_marker(Cell { line, column: 0 }).unwrap();
        let end = session.add_marker(Cell { line, column: 4 }).unwrap();
        session
            .decorations_mut()
            .add(Decoration {
                anchor: Anchor::Range(MarkerRange { start, end }),
                kind: DecorationKind::Highlight { fill: 0xff553311 },
                slot: ZSlot::BelowText,
                auto_remove: true,
                region: Some(RegionSpec::Tooltip("hover annotation".into())),
            })
            .unwrap();
    }
}
