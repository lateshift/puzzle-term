//! Focus API calls release the Session borrow before native focus events run.
use super::*;

/// Native focus callbacks and device-limited fitting cross the rendered host.
#[wasm_bindgen_test(async)]
async fn focus_blur_fit_and_dispose() {
    let _fonts = resources::Fonts::new();
    let (terminal, parent) = resources::create(14).await;
    terminal.focus().unwrap();
    assert!(terminal.has_focus().unwrap());
    terminal.blur().unwrap();
    assert!(!terminal.has_focus().unwrap());
    terminal.fit().unwrap();
    assert!(terminal.owner.borrow().resize.is_some());
    terminal.dispose().unwrap();
    terminal.dispose().unwrap();
    assert!(parent.query_selector("textarea").unwrap().is_none());
    parent.remove();
}

/// Every convenience event crosses the public subscription and disposal surface.
#[wasm_bindgen_test(async)]
async fn all_convenience_events_and_subscription_lifetime() {
    let _fonts = resources::Fonts::new();
    let (terminal, parent) = resources::create(14).await;
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut callbacks = Vec::new();
    let mut handles = Vec::new();
    for name in ["title", "bell", "resize", "focus", "blur", "error"] {
        let seen = seen.clone();
        let callback = Closure::wrap(Box::new(move |_: JsValue| {
            seen.borrow_mut().push(name);
        }) as Box<dyn FnMut(JsValue)>);
        handles.push(
            terminal
                .on(
                    name.into(),
                    callback.as_ref().unchecked_ref::<Function>().clone(),
                )
                .unwrap(),
        );
        callbacks.push(callback);
    }
    terminal.focus().unwrap();
    terminal.blur().unwrap();
    terminal.resize(100.into(), 30.into()).unwrap();
    JsFuture::from(terminal.write("\x1b]2;event-table\x07\x07".into()).unwrap())
        .await
        .unwrap();
    // Dispatch through the registered socket error listener, not directly into the outbox.
    let connection = terminal
        .connect("ws://127.0.0.1:65530".into(), None)
        .unwrap();
    let socket = terminal.owner.borrow().socket.as_ref().unwrap().ws.clone();
    socket
        .dispatch_event(&web_sys::Event::new("error").unwrap())
        .unwrap();
    connection.disconnect();
    for name in ["title", "bell", "resize", "focus", "blur", "error"] {
        assert!(seen.borrow().contains(&name), "missing public event {name}");
    }
    for handle in handles {
        handle.dispose();
    }
    let count = seen.borrow().len();
    terminal.focus().unwrap();
    JsFuture::from(terminal.write("\x1b]2;removed\x07".into()).unwrap())
        .await
        .unwrap();
    assert_eq!(seen.borrow().len(), count);
    terminal.dispose().unwrap();
    parent.remove();
}
