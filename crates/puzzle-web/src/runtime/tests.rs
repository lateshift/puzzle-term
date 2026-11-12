//! Browser API witnesses use real DOM and promises without requiring a GPU.
//! GPU-sharing witnesses have their own resource tests and actual device request.
#![allow(
    clippy::unwrap_used,
    reason = "browser assertions report any unexpected JS or setup failure"
)]
use super::*;
use api::PuzzleTerminal;
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;
/// Refused browser canvas resources must reject creation instead of hanging.
mod creation;
/// Declined cold preparation still paints a visible diagnostic border.
mod deferred;
/// Native focus and disposal must survive synchronous DOM reentrancy.
mod dom;
/// Whole-cluster Canvas2D color/mask ink and retained hidden-frame uploads.
mod fallback;
/// Output, input and subscription disposal all use the public API methods.
mod input;
/// Actual DOM-to-transport latency and 100 MB browser rendering budgets.
mod measurements;
/// Annotation clicks consume input before selection/reporting and own hover cursors.
mod overlays;
/// Public output must put glyph ink in every cell with real HTTP assets.
mod presentation;
/// Writer promises release bounded reply credit only after consumption.
mod reply_credit;
/// Immutable reply buffers resume only on external writer/storage readiness.
mod reply_wait;
/// Actual device/context sharing, release and loss on one browser page.
mod resources;
/// Selection/search API and exact submitted cell pixels.
mod selection_search;
/// Tint validation and actual mask/color atlas pixels guard all pipelines.
mod shaders;
/// Binary browser sockets exercise the real development server.
mod socket;
/// M8 public viewport, pointer and touch behavior with real served assets.
mod viewport;
/// Public construction must surface browser shortcut availability in the log.
mod warnings;

/// Internal headless construction is test support, never an exported M7 API.
fn terminal() -> PuzzleTerminal {
    let owner = Runtime::new(puzzle_term::SessionConfig {
        on_exit: puzzle_term::session::OnExit::Hold,
        ..Default::default()
    });
    PuzzleTerminal { owner }
}

/// A write barrier covers earlier queued output and generated replies.
#[wasm_bindgen_test(async)]
async fn write_resolves_after_parsing_and_reply_flush() {
    let terminal = terminal();
    let bytes = Rc::new(RefCell::new(Vec::new()));
    let sink = bytes.clone();
    let callback = Closure::wrap(Box::new(move |array: js_sys::Uint8Array| {
        sink.borrow_mut().extend(array.to_vec())
    }) as Box<dyn FnMut(js_sys::Uint8Array)>);
    terminal
        .on_data(callback.as_ref().unchecked_ref::<Function>().clone())
        .unwrap();
    let mut output = vec![b'a'; 8 * 1024 * 1024];
    output.extend_from_slice(b"\x1b[5n");
    let promise = terminal
        .write(js_sys::Uint8Array::from(output.as_slice()).into())
        .unwrap();
    JsFuture::from(promise).await.unwrap();
    assert_eq!(terminal.pending_bytes().unwrap(), 0.0);
    assert_eq!(bytes.borrow().as_slice(), b"\x1b[0n");
    JsFuture::from(terminal.writeln("end".into()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        terminal
            .owner
            .borrow()
            .session
            .buffer()
            .cursor()
            .unwrap()
            .column,
        0
    );
    terminal.dispose().unwrap();
    // A completed write must not leave a parser task or subscriber for the next terminal.
    let closed = terminal.owner.borrow();
    assert!(closed.continuation.is_none());
    assert!(closed.raf.is_none() && closed.timer_id.is_none());
    assert!(closed.data.is_empty() && closed.events.is_empty());
    assert!(terminal.write(1.into()).is_err());
}

/// Disposing during a budgeted write cancels its queued task, not just its callback effects.
#[wasm_bindgen_test(async)]
async fn dispose_cancels_queued_parser_work() {
    let terminal = terminal();
    let output = vec![b'a'; 8 * 1024 * 1024];
    let promise = terminal
        .write(js_sys::Uint8Array::from(output.as_slice()).into())
        .unwrap();
    // Attach rejection before disposal so the expected Disposed error is not unhandled.
    let completion = JsFuture::from(promise);
    assert!(terminal.owner.borrow().continuation.is_some());
    let parsed = terminal.owner.borrow().parsed.consumed;
    terminal.dispose().unwrap();
    assert!(
        terminal.owner.borrow().continuation.is_none(),
        "queued parser task survived dispose"
    );
    assert!(completion.await.is_err());
    // A browser task boundary proves canceled work cannot parse or emit anything later.
    socket::delay(0).await;
    let closed = terminal.owner.borrow();
    assert_eq!(closed.parsed.consumed, parsed);
    assert!(closed.waiting.is_empty() && closed.data.is_empty());
}

/// Invalid external values are errors, including cycles and unknown event names.
#[wasm_bindgen_test]
fn invalid_arguments_are_reported() {
    let terminal = terminal();
    assert!(terminal.write(js_sys::Object::new().into()).is_err());
    assert!(terminal.resize(0.into(), 1.into()).is_err());
    for invalid in [f64::NAN, f64::INFINITY, -1.0, 2.5, 65537.0] {
        assert!(terminal.resize(invalid.into(), 24.into()).is_err());
    }
    assert!(terminal.send_key(js_sys::Object::new().into()).is_err());
    let cyclic = js_sys::Object::new();
    js_sys::Reflect::set(&cyclic, &"self".into(), &cyclic).unwrap();
    assert!(value::plain(&cyclic.into(), 0, &mut 4096).is_err());
    terminal.dispose().unwrap();
}

/// Actual Chrome press counts guard word and wrapped-line selection.
mod trusted_clicks;

/// Unicode fallback flood measures actual presentations and child receipt under cold discovery.
mod unicode_flood;
/// Optional release observations use actual CJK/emoji commands, editor, tmux and man.
mod workloads;
