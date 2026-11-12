//! Precharged buffer readiness follows writer progress, never empty host flushes.
use super::*;
use puzzle_term::{
    bus::Bus,
    messages::{Command, Event},
    osc::{OscContext, OscHandler, OscProgress, OscRequest},
    platform::reply::ReusableReply,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// A single metered buffer remains immutable until its writer promise completes.
struct Scoped {
    /// Precharged allocation shared through the browser writer promise.
    buffer: ReusableReply,
    /// Count of committed semantic operations, independent of retries.
    consumed: Arc<AtomicUsize>,
}
/// Trusted registration exercises the generic context without linking PZA.
impl OscHandler for Scoped {
    /// The scoped entry is mandatory for this fixture.
    fn handle(&mut self, _: &OscRequest, _: &mut Vec<u8>, _: &mut Bus<Command, Event>) {
        panic!("fixture requires its scoped context");
    }
    /// Busy storage consumes no semantic operation; actual completion permits retry.
    fn try_handle_with_context(
        &mut self,
        request: &OscRequest,
        context: &mut OscContext<'_>,
        _: &mut Vec<u8>,
        _: &mut Bus<Command, Event>,
    ) -> OscProgress {
        if context
            .reply_from_buffer(&mut self.buffer, &request.payload)
            .is_none()
        {
            return OscProgress::AwaitReply;
        }
        self.consumed.fetch_add(1, Ordering::Relaxed);
        OscProgress::Complete
    }
}

/// A held writer creates no idle continuation, then resumes the exact pending request.
#[wasm_bindgen_test(async)]
async fn precharged_reply_waits_without_empty_flush_polling() {
    let terminal = terminal();
    let consumed = Arc::new(AtomicUsize::new(0));
    terminal
        .owner
        .borrow_mut()
        .session
        .register_osc_handler(
            7779,
            Box::new(Scoped {
                buffer: ReusableReply::new(4096).unwrap(),
                consumed: consumed.clone(),
            }),
        )
        .unwrap();
    let resolvers = Rc::new(RefCell::new(Vec::<Function>::new()));
    let pending = resolvers.clone();
    let writer = Closure::wrap(Box::new(move |_: js_sys::Uint8Array| -> Promise {
        Promise::new(&mut |resolve, _| pending.borrow_mut().push(resolve))
    }) as Box<dyn FnMut(js_sys::Uint8Array) -> Promise>);
    let transport = js_sys::Object::new();
    js_sys::Reflect::set(&transport, &"write".into(), writer.as_ref()).unwrap();
    terminal.attach_transport(transport.into()).unwrap();
    let done = JsFuture::from(
        terminal
            .write("\x1b]7779;one\x1b\\\x1b]7779;two\x1b\\".into())
            .unwrap(),
    );
    socket::delay(0).await;
    assert_eq!(consumed.load(Ordering::Relaxed), 1);
    assert!(terminal.owner.borrow().session.waiting_reply_credit());
    assert!(
        terminal.owner.borrow().continuation.is_none(),
        "an empty flush scheduled a busy retry"
    );
    let first = resolvers.borrow_mut().remove(0);
    first.call0(&JsValue::UNDEFINED).unwrap();
    socket::delay(0).await;
    assert_eq!(consumed.load(Ordering::Relaxed), 2);
    let second = resolvers.borrow_mut().remove(0);
    second.call0(&JsValue::UNDEFINED).unwrap();
    socket::delay(0).await;
    done.await.unwrap();
    assert_eq!(terminal.owner.borrow().session.pending_reply_bytes(), 0);
    terminal.dispose().unwrap();
}
