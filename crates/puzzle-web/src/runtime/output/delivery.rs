//! Foreign callbacks run outside the Session borrow. Reply promises retain credit
//! until actual consumption, independently of bridge or raw socket completion.
//!
//! The Session emits plain owned effects; this module performs browser I/O later.
//! It copies the used payload into a JS array before invoking foreign functions.
//! No RefCell borrow may span an observer or transport callback.
//! The outer flush guard retains FIFO order under synchronous reentrant input.
//! onData is observational; its return cannot acknowledge a blocked writer.
//! There may be a socket, an attached callback, both, or only observers.
//! The route ledger is installed before either actual writer sees this operation.
//! A reply completes only after all actual routes have consumed it.
//! With no actual writer, delivery to the observation API is synchronous.
//! Raw sockets may drain synchronously; bridge routes await actual PTY credit.
//! Callback transports declare synchronous consumption by returning void.
//! A Promise declares that consumption will finish later, outside this JS turn.
//! Its resolution is a writer receipt, not acceptance into another queue.
//! Promise rejection reports failure and returns no reusable reply credit.
//! The host must close a failed transport instead of acknowledging lost bytes.
//! A precharged immutable lease survives across an asynchronous callback writer.
//! Dropping that lease precedes the receipt, permitting safe buffer reuse.
//! Ordinary Vec-backed payloads move into the JS array before callbacks.
//! Pending Promise continuations capture weak ownership, avoiding cycles.
//! Disposal makes a completed Promise inert instead of reviving a dead Session.
//! Completion runs a bounded parser turn without depending on a frame.
//! Hidden pages therefore retain the same reply order as visible pages.
//! Promises can resolve out of order; the route ledger seals the FIFO prefix.
//! Throwing observers are isolated and cannot veto the actual writer handoff.
//! A throwing writer returns its error and does not manufacture completion.
//! The public HostTransport contract documents these responsibilities.
use super::*;
use puzzle_term::platform::reply::ReplyLease;

/// Observers see one immutable snapshot before writer routes consume that operation.
pub(super) fn deliver_write(
    owner: &Owner,
    bytes: WriteData,
    callbacks: Vec<(u64, Function)>,
    transport: Option<JsValue>,
    reply: Option<u64>,
) -> Result<(), JsValue> {
    let array = js_sys::Uint8Array::from(bytes.as_slice());
    observe(owner, &array, callbacks);
    if owner.borrow().disposed {
        return Ok(());
    }
    // Create the route ledger before a synchronous raw socket can return credit.
    if let Some(id) = reply {
        let mut runtime = owner.borrow_mut();
        let routes = usize::from(runtime.socket.is_some()) + usize::from(transport.is_some());
        runtime.reply_routes.push_back((id, routes.max(1)));
    }
    let lease = bytes.lease();
    let deferred = socket::write(owner, bytes, reply)?;
    if let Some(transport) = transport {
        host_write(owner, transport, &array, reply, lease)?;
    } else if let Some(id) = reply.filter(|_| !deferred) {
        drop(lease);
        owner
            .borrow_mut()
            .complete_reply_route(id)
            .map_err(|e| value::error("TransportError", &e))?;
    }
    Ok(())
}

/// Throwing observers cannot veto encoded input or keep a live mutable core borrow.
fn observe(owner: &Owner, array: &js_sys::Uint8Array, callbacks: Vec<(u64, Function)>) {
    for (_, callback) in callbacks {
        if owner.borrow().disposed {
            break;
        }
        if let Err(error) = callback.call1(&JsValue::UNDEFINED, array) {
            tracing::warn!(event = "web.data_callback_failed", ?error);
        }
    }
}

/// Void means synchronous consumption; a returned Promise means actual later write.
fn host_write(
    owner: &Owner,
    transport: JsValue,
    array: &js_sys::Uint8Array,
    reply: Option<u64>,
    lease: Option<ReplyLease>,
) -> Result<(), JsValue> {
    let method = value::get(&transport, "write")?.dyn_into::<Function>()?;
    let result = method.call1(&transport, array)?;
    if let Some(id) = reply
        && let Ok(promise) = result.dyn_into::<Promise>()
    {
        complete_later(owner, promise, id, lease);
        return Ok(());
    }
    // Release immutable storage before telling a waiting producer it may reuse it.
    drop(lease);
    if let Some(id) = reply {
        owner
            .borrow_mut()
            .complete_reply_route(id)
            .map_err(|e| value::error("TransportError", &e))?;
    }
    Ok(())
}

/// Disposing revokes completion; a failed writer never manufactures success credit.
fn complete_later(owner: &Owner, promise: Promise, id: u64, lease: Option<ReplyLease>) {
    let weak = Rc::downgrade(owner);
    wasm_bindgen_futures::spawn_local(async move {
        let result = wasm_bindgen_futures::JsFuture::from(promise).await;
        drop(lease);
        if let Some(owner) = weak.upgrade().filter(|o| !o.borrow().disposed) {
            if result.is_ok() {
                if let Err(error) = owner.borrow_mut().complete_reply_route(id) {
                    tracing::warn!(event = "web.reply_completion_failed", %error);
                }
                // This is a bounded parser turn, not a wait on rendering or visibility.
                let _ = feed(&owner, &[], None);
            } else {
                events::emit(
                    &owner,
                    "error",
                    value::error("TransportError", "reply writer rejected completion"),
                );
            }
        }
    });
}
