//! A stalled asynchronous JS writer must throttle only its own program output.
use super::*;
use puzzle_term::platform::reply::PENDING_BYTE_ALLOWANCE;

/// Actual consumption resumes exact replies; a key remains serviceable without a frame.
#[wasm_bindgen_test(async)]
async fn reply_credit_waits_for_writer_promises_without_polling_or_losing_output() {
    let terminal = terminal();
    let resolvers = Rc::new(RefCell::new(Vec::<Function>::new()));
    let bytes = Rc::new(RefCell::new(Vec::new()));
    let (pending, sink) = (resolvers.clone(), bytes.clone());
    let writer = Closure::wrap(Box::new(move |array: js_sys::Uint8Array| -> Promise {
        sink.borrow_mut().extend(array.to_vec());
        Promise::new(&mut |resolve, _| pending.borrow_mut().push(resolve))
    }) as Box<dyn FnMut(js_sys::Uint8Array) -> Promise>);
    let transport = js_sys::Object::new();
    js_sys::Reflect::set(&transport, &"write".into(), writer.as_ref()).unwrap();
    terminal.attach_transport(transport.into()).unwrap();
    let queries = PENDING_BYTE_ALLOWANCE / b"\x1b[?62;22c".len() + 1000;
    let completion = JsFuture::from(terminal.write("\x1b[c".repeat(queries).into()).unwrap());
    // Let budgeted parser continuations reach their structural writer boundary.
    while terminal.owner.borrow().continuation.is_some() {
        socket::delay(0).await;
    }
    let parsed = terminal.owner.borrow().parsed.consumed;
    assert!(terminal.owner.borrow().session.waiting_reply_credit());
    assert!(terminal.owner.borrow().session.pending_reply_bytes() <= PENDING_BYTE_ALLOWANCE);
    socket::delay(0).await;
    assert_eq!(terminal.owner.borrow().parsed.consumed, parsed);
    assert!(terminal.owner.borrow().continuation.is_none());
    terminal.input("K".into()).unwrap();
    let key_offset = bytes.borrow().len() - 1;
    assert_eq!(bytes.borrow()[key_offset], b'K');
    // Resolve all actual writer operations; newly admitted batches may add more.
    loop {
        let batch = std::mem::take(&mut *resolvers.borrow_mut());
        for resolve in batch {
            resolve.call0(&JsValue::UNDEFINED).unwrap();
        }
        socket::delay(0).await;
        if terminal.owner.borrow().session.pending_bytes() == 0
            && terminal.owner.borrow().session.pending_reply_bytes() == 0
        {
            break;
        }
    }
    completion.await.unwrap();
    let mut expected = b"\x1b[?62;22c".repeat(queries);
    expected.insert(key_offset, b'K');
    assert_eq!(*bytes.borrow(), expected);
    assert!(terminal.owner.borrow().reply_routes.is_empty());
    terminal.dispose().unwrap();
}
