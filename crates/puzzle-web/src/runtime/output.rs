//! Each output promise waits for its parsed prefix and all resulting replies.
//!
//! Parsing never waits for a frame or a visible page.
//! The accepted byte offset and parsed byte offset describe one ordered
//! application-output stream, even when a page write and WebSocket output
//! interleave. A write promise stores its final accepted offset. Its prefix
//! can resolve while a later write remains queued; a later resolver can
//! never overtake an earlier unparsed prefix.
//!
//! The parser's time budget limits each feed call rather than the entire
//! output stream. A suffix is retained in Session and one event-loop timer
//! continues it. This lets input handlers and refresh callbacks run between
//! parse turns instead of freezing a page during a large application write.
//!
//! Replies such as DA and CPR are generated during parsing. They enter the
//! same deferred transport FIFO as keys and paste data. Flushing that FIFO
//! before resolving promises ensures an embedding host observes the reply
//! before write's completion, rather than receiving it at the next frame.
//!
//! Output credit belongs to the socket that supplied the bytes. The ledger
//! therefore keeps a connection identity alongside each pending span. An
//! API-injected diagnostic message does not acknowledge bridge output, and
//! an old connection's suffix does not grant credit to a new connection.
//!
//! User callbacks execute after the mutable core borrow is released. They
//! may enqueue new output or remove themselves; the next loop iteration
//! checks disposal before invoking anything else. Promise callbacks remain
//! owned until completion or rejection, so disposal does not strand a waiter.
//!
//! A bridge exit is delivered after the final queued output has been parsed,
//! not merely after its socket packet was received. The held exit notice can
//! then follow the application's final line without hiding an unread suffix.
//! Only the presentation of that state waits for rAF; parsing and replies
//! continue even when the document is hidden.
//! Continuations use one event-loop turn at a time so a large write leaves
//! input and rendering fair.
//! A callback transport is an application integration, not a bridge server.
//! Its write callback can enqueue a key synchronously without awaiting a frame.
//! The output promise still belongs to the original parser prefix, so that
//! reentrant input cannot resolve an unrelated unparsed output write.
//!
//! The zero-byte write barrier is useful to await an already accepted stream.
//! It creates no provenance span and consumes no server credit, but it still
//! waits for preceding output and reply delivery before resolving.
//!
//! Continuation callbacks retain only weak ownership. Disposing the terminal
//! while one is queued makes it a no-op; it cannot recreate a Session or mount
//! a new textarea. Promise rejection explicitly tells the host work has stopped.
//!
//! The parse count may include bytes accepted by an earlier call. Attributing
//! only the current packet's length would ACK unparsed data or miss a prefix.
//! The ledger walks original spans instead of guessing from message boundaries.
//!
//! Held exit notices are core cell writes, not synthetic PTY output. They do
//! not consume output credit and cannot be echoed back into the child.
use super::*;
use wasm_bindgen::{JsCast, closure::Closure};
/// Input Vecs and immutable precharged reply leases share the transport seam.
mod data;
/// Callback delivery separates host promises from socket writer receipts.
mod delivery;
pub(super) use data::WriteData;
/// Several writer routes complete one Session receipt, in source order.
mod replies;
use delivery::deliver_write;

/// A completion callback is owned until its prefix has actually been parsed.
pub(super) struct Barrier {
    /// Accepted stream offset, including output preceding this write.
    end: u64,
    /// Promise resolver is invoked after reply callbacks, not before them.
    resolve: Function,
    /// Disposal rejects work that can no longer be parsed.
    reject: Function,
}

/// Feeds output with an explicit completion barrier and bounded core budget.
/// Output received from a socket uses the same budget and provenance as API writes.
pub(super) fn feed(owner: &Owner, bytes: &[u8], connection: Option<u64>) -> Result<(), JsValue> {
    // Attribute accepted spans before parsing can synchronously consume older queued bytes.
    let acks = {
        let mut r = owner.borrow_mut();
        r.ensure_live()?;
        r.parsed.accept(bytes.len(), connection);
        if !bytes.is_empty() {
            let now = r.started.elapsed();
            let period = r.period;
            r.settle.request(now, period);
        }
        let outcome = r.session.feed(bytes, Instant::now());
        #[cfg(any(test, feature = "measurements"))]
        if connection.is_some() {
            // Observe the accepted suffix here, before ACKs permit another packet.
            let pending = r.session.pending_bytes();
            r.measurements.output(bytes, pending);
        }
        r.parsed
            .advance(outcome.parsed_bytes)
            .map_err(|e| value::error("TransportError", e))?
    };
    // Only consumed output returns server credit; packet receipt is not sufficient.
    socket::ack(owner, acks)?;
    flush(owner)?;
    // Replies and host effects precede promise completion for this accepted prefix.
    complete(owner);
    continue_later(owner)?;
    draw::schedule(owner)?;
    finish_exit(owner)
}

/// A bridge exit cannot overtake a budget-limited suffix of final output.
pub(super) fn finish_exit(owner: &Owner) -> Result<(), JsValue> {
    {
        let mut r = owner.borrow_mut();
        if r.session.pending_bytes() != 0 {
            return Ok(());
        }
        if let Some(info) = r.exit.take() {
            r.session.submit(
                puzzle_term::messages::Command::ChildExited(info),
                puzzle_term::Origin::Platform,
            );
            r.session.pump(Instant::now());
        }
    }
    flush(owner)?;
    draw::schedule(owner)
}

/// Returns a promise tied to this prefix; replies flush before its resolver.
pub(crate) fn write(owner: &Owner, bytes: Vec<u8>) -> Result<Promise, JsValue> {
    owner.borrow().ensure_live()?;
    let weak = Rc::downgrade(owner);
    let mut bytes = Some(bytes);
    // Promise executors run synchronously, so the barrier is installed before parsing begins.
    let promise = Promise::new(&mut move |resolve, reject| {
        if let Some(owner) = weak.upgrade() {
            let mut runtime = owner.borrow_mut();
            // Ownership transfers once into the parser; a retained promise does not clone output.
            let data = bytes.take().unwrap_or_default();
            let end = runtime.parsed.accepted + data.len() as u64;
            // This offset includes preceding writes and therefore cannot overtake their suffixes.
            runtime.waiting.push_back(Barrier {
                end,
                resolve,
                reject: reject.clone(),
            });
            drop(runtime);
            // The ordinary feed lane owns provenance, parser errors, reply flush and completion.
            if let Err(error) = feed(&owner, &data, None) {
                let _ = reject.call1(&JsValue::UNDEFINED, &error);
                events::emit(&owner, "error", error);
            }
        }
    });
    flush(owner)?;
    // Replies and host effects precede promise completion for this accepted prefix.
    complete(owner);
    continue_later(owner)?;
    draw::schedule(owner)?;
    Ok(promise)
}

/// Flushes typed effects after releasing the Session borrow, allowing reentrancy.
pub(crate) fn flush(owner: &Owner) -> Result<(), JsValue> {
    // Foreign observers may enqueue keys recursively; their effects remain behind this one.
    {
        let mut runtime = owner.borrow_mut();
        if runtime.flushing {
            return Ok(());
        }
        runtime.flushing = true;
    }
    // Always release the delivery guard, including a throwing transport callback.
    let result = flush_effects(owner);
    // The flag describes callback nesting, not whether any effect succeeded.
    // A later input can retry delivery after a failed host write reports its error.
    owner.borrow_mut().flushing = false;
    result
}

/// Delivers the FIFO under a reentrancy guard while keeping each core borrow short.
fn flush_effects(owner: &Owner) -> Result<(), JsValue> {
    loop {
        let (effect, callbacks, transport) = {
            let mut runtime = owner.borrow_mut();
            if runtime.disposed {
                return Ok(());
            }
            (
                runtime.take_transport_effect(),
                runtime.data.clone(),
                runtime.transport.clone(),
            )
        };
        // Drain one owned effect per iteration so callback reentrancy cannot hold a Session borrow.
        match effect {
            Some(TransportEffect::Write(bytes)) => {
                deliver_write(owner, WriteData::Owned(bytes), callbacks, transport, None)?
            }
            Some(TransportEffect::Reply(reply)) => {
                deliver_write(
                    owner,
                    reply.bytes.into(),
                    callbacks,
                    transport,
                    Some(reply.ticket.id()),
                )?;
            }
            Some(TransportEffect::Resize(size)) => {
                socket::resize(owner)?;
                if let Some(transport) = transport
                    && let Ok(method) = value::get(&transport, "resize")?.dyn_into::<Function>()
                {
                    let value = js_sys::Object::new();
                    js_sys::Reflect::set(&value, &"cols".into(), &size.columns.into())?;
                    js_sys::Reflect::set(&value, &"rows".into(), &size.rows.into())?;
                    // Host transports receive the same physical surface facts as grid fitting.
                    js_sys::Reflect::set(&value, &"widthPx".into(), &size.pixel_width.into())?;
                    js_sys::Reflect::set(&value, &"heightPx".into(), &size.pixel_height.into())?;
                    method.call1(&transport, &value)?;
                }
            }
            Some(TransportEffect::Close) => {
                owner.borrow_mut().socket = None;
                if let Some(transport) = transport
                    && let Ok(close) = value::get(&transport, "close")?.dyn_into::<Function>()
                {
                    close.call0(&transport)?;
                }
            }
            // Facts follow the completed turn, after every reply/input effect has been handed off.
            None => {
                events::flush(owner);
                return Ok(());
            }
        }
    }
}

/// Resolves only mature barriers; a callback may have added a later write.
pub(super) fn complete(owner: &Owner) {
    loop {
        let next = {
            let mut runtime = owner.borrow_mut();
            if runtime
                .waiting
                .front()
                .is_none_or(|b| b.end > runtime.parsed.consumed)
            {
                return;
            }
            runtime.waiting.pop_front()
        };
        // Resolve outside the mutable borrow, even though JS handlers run in a later microtask.
        if let Some(barrier) = next {
            let _ = barrier.resolve.call0(&JsValue::UNDEFINED);
        }
    }
}

/// Owns a browser task and its Rust callback until it fires or is canceled.
/// A weak capture prevents an ownership cycle, but does not cancel setTimeout.
/// Keeping both resources here lets dispose release them without waiting for JS.
pub(super) struct Continuation {
    /// Browser handle permits cancellation before dropping the callable closure.
    id: i32,
    /// Unlike once_into_js, an owned closure can be freed even if it never runs.
    _callback: Closure<dyn FnMut()>,
}

/// Teardown cancels the browser task before invalidating its callable Rust target.
impl Drop for Continuation {
    /// A destroyed page has no remaining task queue; a live page must cancel this ID.
    fn drop(&mut self) {
        if let Ok(window) = window() {
            window.clear_timeout_with_handle(self.id);
        }
    }
}

/// Schedules one owned continuation only while the parser retains accepted bytes.
fn continue_later(owner: &Owner) -> Result<(), JsValue> {
    let mut runtime = owner.borrow_mut();
    if runtime.disposed
        || runtime.continuation.is_some()
        || runtime.session.waiting_reply_credit()
        || runtime.parsed.consumed == runtime.parsed.accepted
            && runtime.session.pending_bytes() == 0
    {
        return Ok(());
    }
    // One task parses the suffix; repeated feeds cannot schedule a storm of continuation timers.
    let weak = Rc::downgrade(owner);
    // An owned one-shot can be canceled, unlike a JS-owned callback whose handle was discarded.
    let callback = Closure::once(move || {
        if let Some(owner) = weak.upgrade() {
            {
                let mut runtime = owner.borrow_mut();
                // Dropping an executing closure is deferred by wasm-bindgen's invocation guard.
                runtime.continuation = None;
                if runtime.disposed {
                    return;
                }
                // Feed an empty slice to continue the retained FIFO under the same time budget.
                let outcome = runtime.session.feed(&[], Instant::now());
                let acks = runtime
                    .parsed
                    .advance(outcome.parsed_bytes)
                    .unwrap_or_default();
                drop(runtime);
                if let Err(error) = socket::ack(&owner, acks) {
                    tracing::warn!(event = "web.ack_failed", ?error);
                }
            }
            if let Err(error) = flush(&owner) {
                tracing::warn!(event = "web.transport_failed", ?error);
            }
            complete(&owner);
            let _ = finish_exit(&owner);
            let _ = draw::schedule(&owner);
            if let Err(error) = continue_later(&owner) {
                tracing::warn!(event = "web.parse_schedule_failed", ?error);
            }
        }
    });
    let id = window()?.set_timeout_with_callback_and_timeout_and_arguments_0(
        callback.as_ref().unchecked_ref(),
        0,
    )?;
    runtime.continuation = Some(Continuation {
        id,
        _callback: callback,
    });
    Ok(())
}

/// Disposal cannot leave callers waiting forever on a dead parser.
pub(super) fn reject(pending: VecDeque<Barrier>) {
    for barrier in pending {
        let _ = barrier.reject.call1(
            &JsValue::UNDEFINED,
            &value::error("Disposed", "terminal was disposed"),
        );
    }
}

/// Host lifecycle cleanup is foreign code and must run without a core borrow.
pub(super) fn close_transport(transport: Option<JsValue>) -> Result<(), JsValue> {
    if let Some(transport) = transport {
        let method = value::get(&transport, "close")?;
        if method.is_function() {
            // Preserve this binding for class-based host transport implementations.
            method.dyn_into::<Function>()?.call0(&transport)?;
        }
    }
    Ok(())
}
