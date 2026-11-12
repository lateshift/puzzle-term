//! Browser resource refusal must become a clear public create error.
//!
//! A canvas with a 2D context returns null for WebGPU, the same refusal
//! surface as an exhausted browser canvas resource pool. No fixed canvas
//! count is assumed: retained histories can exhaust wasm before the GPU.
use super::*;

/// Models browser getContext refusal through the real public create path.
#[wasm_bindgen_test(async)]
async fn refused_canvas_rejects_create_with_clear_error() {
    let _fonts = resources::Fonts::new();
    let document = window().unwrap().document().unwrap();
    let host = document.create_element("div").unwrap().dyn_into().unwrap();
    let canvas = document
        .create_element("canvas")
        .unwrap()
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .unwrap();
    // The browser itself supplies null, rather than a fabricated renderer error.
    assert!(canvas.get_context("2d").unwrap().is_some());
    let original = value::get(document.as_ref(), "createElement")
        .unwrap()
        .dyn_into::<Function>()
        .unwrap();
    let page = document.clone();
    let callback = Closure::wrap(Box::new(move |tag: String| -> JsValue {
        if tag == "canvas" {
            canvas.clone().into()
        } else {
            original.call1(page.as_ref(), &tag.into()).unwrap()
        }
    }) as Box<dyn FnMut(String) -> JsValue>);
    js_sys::Reflect::set(
        document.as_ref(),
        &"createElement".into(),
        callback.as_ref(),
    )
    .unwrap();
    let result = PuzzleTerminal::create(host, None).await;
    // Remove our own override before any assertion, restoring the native prototype.
    js_sys::Reflect::delete_property(
        document.unchecked_ref::<js_sys::Object>(),
        &"createElement".into(),
    )
    .unwrap();
    let error = match result {
        Err(error) => error,
        Ok(terminal) => {
            terminal.dispose().unwrap();
            panic!("create accepted a refused browser canvas");
        }
    };
    assert_eq!(value::get(&error, "code").unwrap(), "WebGpuUnavailable");
    let message = value::get(&error, "message").unwrap().as_string().unwrap();
    assert!(message.contains("getContext() returned null"), "{message}");
}
