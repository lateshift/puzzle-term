//! Host input and convenience events cannot wait for a render callback.
use super::*;

/// Encoded keys, sanitized paste and parser replies leave in the same FIFO.
#[wasm_bindgen_test(async)]
async fn input_paste_send_key_and_disposable() {
    let terminal = terminal();
    let received = Rc::new(RefCell::new(Vec::<u8>::new()));
    let sink = received.clone();
    let callback = Closure::wrap(Box::new(move |bytes: js_sys::Uint8Array| {
        sink.borrow_mut().extend(bytes.to_vec())
    }) as Box<dyn FnMut(js_sys::Uint8Array)>);
    let subscription = terminal
        .on_data(callback.as_ref().unchecked_ref::<Function>().clone())
        .unwrap();
    terminal.input("a".into()).unwrap();
    JsFuture::from(terminal.write("\x1b[?2004h".into()).unwrap())
        .await
        .unwrap();
    terminal.paste("paste".into()).unwrap();
    let key = js_sys::Object::new();
    js_sys::Reflect::set(&key, &"key".into(), &"d".into()).unwrap();
    js_sys::Reflect::set(&key, &"ctrl".into(), &true.into()).unwrap();
    terminal.send_key(key.into()).unwrap();
    assert_eq!(
        received.borrow().as_slice(),
        b"a\x1b[200~paste\x1b[201~\x04"
    );
    subscription.dispose();
    terminal.input("ignored".into()).unwrap();
    assert_eq!(received.borrow().last(), Some(&4));
    terminal.dispose().unwrap();
}

/// Callback transport replies are flushed before promise completion and can reenter.
#[wasm_bindgen_test(async)]
async fn callback_transport_and_events() {
    let terminal = terminal();
    let received = Rc::new(RefCell::new(Vec::<u8>::new()));
    let sink = received.clone();
    let callback = Closure::wrap(Box::new(move |bytes: js_sys::Uint8Array| {
        sink.borrow_mut().extend(bytes.to_vec())
    }) as Box<dyn FnMut(js_sys::Uint8Array)>);
    let transport = js_sys::Object::new();
    js_sys::Reflect::set(&transport, &"write".into(), callback.as_ref()).unwrap();
    let disposable = terminal.attach_transport(transport.into()).unwrap();
    let titles = Rc::new(RefCell::new(Vec::new()));
    let sink = titles.clone();
    let title = Closure::wrap(Box::new(move |value: JsValue| {
        sink.borrow_mut().push(value.as_string().unwrap())
    }) as Box<dyn FnMut(JsValue)>);
    let subscription = terminal
        .on(
            "title".into(),
            title.as_ref().unchecked_ref::<Function>().clone(),
        )
        .unwrap();
    JsFuture::from(terminal.write("\x1b]2;fixture\x07\x1b[5n".into()).unwrap())
        .await
        .unwrap();
    assert_eq!(received.borrow().as_slice(), b"\x1b[0n");
    assert_eq!(titles.borrow().as_slice(), &["fixture"]);
    disposable.dispose();
    subscription.dispose();
    terminal.input("not sent".into()).unwrap();
    let info = js_sys::Object::new();
    js_sys::Reflect::set(&info, &"code".into(), &3.into()).unwrap();
    terminal.exited(info.into()).unwrap();
    assert_eq!(
        terminal.owner.borrow().session.exit_info().unwrap().code,
        Some(3)
    );
    assert_eq!(terminal.cols().unwrap(), 80);
    assert_eq!(terminal.rows().unwrap(), 24);
    terminal.dispose().unwrap();
}

/// Reentrant onData cannot send a nested key before the current host write.
#[wasm_bindgen_test]
fn reentrant_input_preserves_transport_fifo() {
    let terminal = terminal();
    let weak = Rc::downgrade(&terminal.owner);
    let observer = Closure::wrap(Box::new(move |bytes: js_sys::Uint8Array| {
        if bytes.to_vec() == b"a" {
            PuzzleTerminal {
                owner: weak.upgrade().unwrap(),
            }
            .input("b".into())
            .unwrap();
        }
    }) as Box<dyn FnMut(js_sys::Uint8Array)>);
    terminal
        .on_data(observer.as_ref().unchecked_ref::<Function>().clone())
        .unwrap();
    let received = Rc::new(RefCell::new(Vec::<u8>::new()));
    let sink = received.clone();
    let writer = Closure::wrap(Box::new(move |bytes: js_sys::Uint8Array| {
        sink.borrow_mut().extend(bytes.to_vec());
    }) as Box<dyn FnMut(js_sys::Uint8Array)>);
    let transport = js_sys::Object::new();
    js_sys::Reflect::set(&transport, &"write".into(), writer.as_ref()).unwrap();
    terminal.attach_transport(transport.into()).unwrap();
    terminal.input("a".into()).unwrap();
    assert_eq!(received.borrow().as_slice(), b"ab");
    // Legacy release is an intentionally ignored event, not an invalid key.
    let spec = js_sys::Object::new();
    js_sys::Reflect::set(&spec, &"key".into(), &"d".into()).unwrap();
    js_sys::Reflect::set(&spec, &"type".into(), &"release".into()).unwrap();
    terminal.send_key(spec.into()).unwrap();
    assert_eq!(received.borrow().as_slice(), b"ab");
    terminal.dispose().unwrap();
}

/// Browser default ownership follows XTSHIFTESCAPE without a rendering turn.
#[wasm_bindgen_test(async)]
async fn shift_capture_dom_policy_reads_session_request() {
    use crate::dom::DomHost;
    let terminal = terminal();
    let host = super::super::host::Host(Rc::downgrade(&terminal.owner));
    assert!(host.shift_selects());
    for (bytes, selects) in [
        ("\x1b[>1s", false),
        ("\x1b[>0s", true),
        ("\x1b[>1s\x1bc", true),
    ] {
        JsFuture::from(terminal.write(bytes.into()).unwrap())
            .await
            .unwrap();
        assert_eq!(host.shift_selects(), selects);
    }
    terminal.dispose().unwrap();
}
