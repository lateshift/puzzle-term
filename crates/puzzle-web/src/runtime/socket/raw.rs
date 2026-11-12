//! Raw WebSockets have no application-credit frame. A completed browser send
//! buffer is their observable writer boundary; the remote host remains responsible
//! for its own PTY backpressure. Outstanding reply credit is retained while the
//! browser still owns unsent bytes. User input shares that socket's FIFO.
//!
//! WebSocket has no buffered-amount-low event.
//!
//! One drain observer serves all outstanding raw-socket reply tickets.
//! Zero buffered_amount means the browser no longer owns those unsent bytes.
//! It cannot prove remote PTY delivery: raw peers have no ACK primitive.
//! The bridge uses its stronger PTY-consumption credit, without this observer.
//! Immutable leases survive while browser writes remain pending.
//! Drain drops those leases before returning the owner receipts.
//! A producer cannot overwrite a payload while another writer route uses it.
//! Observing a bridge or empty queue creates no timer.
//! Nonzero buffering installs exactly one continuation for that socket.
//! The callback clears its handle before deciding whether to reschedule.
//! Its next turn is event-loop scheduling, not a tuned polling interval.
//! Socket close drops Timer and cancels the pending browser invocation.
//! Weak, connection-scoped captures cannot touch a replacement connection.
//! A disposed owner exits before doing I/O or scheduling parser work.
//! Writer credit is separate from application-output acknowledgements.
//! Parsing resumes after all pending raw writes have drained.
//! Socket FIFO preserves input order without another OS writer.
//! No callback borrows Session across browser functions or invokes rendering.
//! Reuse zero-delay continuations only while writes are outstanding; idle sockets have no
//! timer. This never polls Session or requests rendering on a hidden page.
use super::*;
use std::collections::VecDeque;

/// Reply receipts and at most one browser continuation belong to one socket.
#[derive(Default)]
pub(super) struct Drain {
    /// In FIFO order; no payload bytes are copied into this observation queue.
    ids: VecDeque<(u64, Option<puzzle_term::platform::reply::ReplyLease>)>,
    /// Destroyed with the socket, before an obsolete callback can touch a new one.
    timer: Option<Timer>,
}
impl Drain {
    /// Admission already charged these bytes on Session's shared reply lane.
    pub(super) fn push(
        &mut self,
        id: u64,
        lease: Option<puzzle_term::platform::reply::ReplyLease>,
    ) {
        self.ids.push_back((id, lease));
    }
}

/// Browser lifetime and Rust callback must be cancelled together.
struct Timer {
    /// Host handle is valid only for this pending browser task.
    id: i32,
    /// Retained Rust trampoline; invocation uses wasm-bindgen's executing guard.
    _callback: Closure<dyn FnMut()>,
}
impl Drop for Timer {
    /// Closing cancels observation without completing any unconsumed reply.
    fn drop(&mut self) {
        if let Ok(window) = window() {
            window.clear_timeout_with_handle(self.id);
        }
    }
}

/// Returns receipts only when the browser has consumed all preceding FIFO bytes.
pub(super) fn observe(owner: &Owner) -> Result<(), JsValue> {
    let ids = {
        let mut r = owner.borrow_mut();
        let Some(socket) = r.socket.as_mut().filter(|s| !s.bridge) else {
            return Ok(());
        };
        if socket.raw.ids.is_empty() {
            return Ok(());
        }
        if socket.ws.buffered_amount() == 0 {
            socket.raw.timer = None;
            socket.raw.ids.drain(..).collect::<Vec<_>>()
        } else {
            if socket.raw.timer.is_none() {
                socket.raw.timer = Some(schedule(owner, socket.id)?);
            }
            return Ok(());
        }
    };
    for (id, lease) in ids {
        drop(lease);
        // Owner-local validation still rejects an obsolete or invented receipt.
        owner
            .borrow_mut()
            .complete_reply_route(id)
            .map_err(|e| value::error("TransportError", &e))?;
    }
    Ok(())
}

/// No timing margin is introduced: yield to the next ordinary browser task turn.
fn schedule(owner: &Owner, socket_id: u64) -> Result<Timer, JsValue> {
    let weak = Rc::downgrade(owner);
    let callback = Closure::once(move || {
        if let Some(owner) = weak.upgrade().filter(|owner| !owner.borrow().disposed) {
            let active = {
                let mut r = owner.borrow_mut();
                r.socket
                    .as_mut()
                    .filter(|s| s.id == socket_id)
                    .map(|s| s.raw.timer = None)
                    .is_some()
            };
            if !active {
                return;
            }
            if let Err(error) = observe(&owner) {
                events::emit(&owner, "error", error);
                return;
            }
            let waiting = owner
                .borrow()
                .socket
                .as_ref()
                .is_some_and(|s| !s.raw.ids.is_empty());
            if !waiting {
                // A completion wakes the bounded parser turn, never a frame dependency.
                let _ = output::feed(&owner, &[], None);
            }
        }
    });
    let id = window()?.set_timeout_with_callback_and_timeout_and_arguments_0(
        callback.as_ref().unchecked_ref(),
        0,
    )?;
    Ok(Timer {
        id,
        _callback: callback,
    })
}
