//! Raw sockets and the development bridge share immediate ordered input (§12.3).
//!
//! The bridge always reads its socket: input credit holds paste tails here,
//! while resize and output ACK messages bypass that byte queue. The output
//! ledger returns credit only for bytes the Session has actually parsed.
//!
//! There are two independent credit directions. The server's output window
//! is replenished when Session parses application output. Its input window
//! is replenished when bytes are actually written to the PTY, not when the
//! socket merely receives a paste. Neither grant is evidence that the other
//! direction has made progress.
//!
//! Without input credit a server that pauses socket reads can strand output
//! ACK packets behind an oversized paste. An echoing program then blocks
//! writing stdout and stops consuming stdin, producing a permanent cycle.
//! Here the unsent paste stays on the client and control packets stay sendable.
//!
//! A socket incarnation has a monotonically assigned identity. Its callbacks
//! check that identity before feeding output or closing the current transport.
//! A delayed close from an old connection cannot interrupt a later reconnect.
//! Likewise, parsing an old output span never ACKs the new application.
//!
//! The browser delivers one message at a time. Binary bridge frames are
//! validated by the shared codec before they change any ledger or grid state.
//! Unknown tags, incorrect lengths and invalid credit grants become transport
//! errors and close the connection; bytes are not interpreted partially.
//!
//! Input messages contain already encoded bytes. The Session has applied
//! keyboard modes, paste sanitization and policy before those bytes arrive
//! here. Splitting a write into codec-sized packets cannot change its order
//! relative to a later key or a parser-generated DA or CPR reply.
//!
//! Hello communicates appearance before the child starts. The following
//! resize reports the currently fitted grid rather than assuming 80 by 24.
//! Raw transports deliberately omit these development-protocol controls;
//! embedding hosts supply their own framing and exit notification contract.
//!
//! Closure ownership is explicit because assigning an onmessage property
//! does not make a wasm trampoline safe after its Rust Closure is dropped.
//! Disconnect first removes all properties, then releases the callbacks.
//! The socket close asks the local dev bridge to terminate and reap its child.
//!
//! Errors are reported as public error events, with the same stable code as
//! synchronous API validation. No token or complete connection URL is logged.
//! The bridge URL is supplied by the page and is never a shipped backend.
//! Ready state is sampled at each send. A raw socket that has not opened
//! cannot silently swallow an encoded key; the host receives a transport error.
//! Bridge input can wait before hello because its credit ledger owns those bytes.
//!
//! A server grant is validated before changing the balance. Zero, overflow
//! and grants beyond the one-megabyte window cannot create send permission.
//! The queued paste remains untouched when such a packet is rejected.
//!
//! Splitting input is byte-oriented, not UTF-8-oriented. PTY reads and writes
//! already preserve byte order across boundaries, and the terminal parser on
//! the application side accepts a Unicode sequence spanning several chunks.
//!
//! Output bytes are not ACKed by the WebSocket onmessage handler just because
//! it copied an ArrayBuffer. Session's consumed count supplies that evidence,
//! including bytes parsed by budget continuations while no frame is requested.
//!
//! An exit packet is not a close packet. It keeps the canvas and held final
//! screen available until the page or Session close policy removes it.
//! Disconnecting the socket alone does not erase the application's final grid.
use super::*;
use puzzle_term::bridge::{self, Client, Server};
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{Event, MessageEvent, WebSocket};
/// Raw sockets have no remote credit frame; their browser send buffer is the writer.
mod raw;

/// Event listeners belong to one connection and are removed on disconnect.
pub(super) struct Socket {
    /// Browser TCP/WebSocket buffering supplies transport delivery.
    pub(super) ws: WebSocket,
    /// Raw hosts do not use the development codec or its credit ledger.
    bridge: bool,
    /// A stale callback cannot feed a replacement session connection.
    pub(super) id: u64,
    /// Unsent input preserves paste → key → parser-reply FIFO order.
    credit: crate::transport::credit::InputCredit,
    /// Plain browser event closures are retained until their handlers are removed.
    listeners: Vec<Closure<dyn FnMut(Event)>>,
    /// One continuation observes actual browser writer drain, only while bytes wait.
    raw: raw::Drain,
}

/// Closing removes browser callbacks before releasing their trampolines.
impl Drop for Socket {
    fn drop(&mut self) {
        // Detach browser properties before releasing Rust trampolines for this incarnation.
        self.ws.set_onopen(None);
        self.ws.set_onmessage(None);
        self.ws.set_onerror(None);
        self.ws.set_onclose(None);
        let _ = self.ws.close();
    }
}

/// Opens only a client transport; Puzzle Terminal never ships a backend (§12.5).
pub(super) fn connect(owner: &Owner, url: &str, bridge: bool) -> Result<u64, JsValue> {
    owner.borrow().ensure_live()?;
    let ws = WebSocket::new(url)?;
    // ArrayBuffer delivery avoids asynchronous Blob conversion reordering output packets.
    ws.set_binary_type(web_sys::BinaryType::Arraybuffer);
    let id = {
        let mut r = owner.borrow_mut();
        r.next_id += 1;
        r.next_id
    };
    let mut socket = Socket {
        ws: ws.clone(),
        bridge,
        id,
        credit: Default::default(),
        listeners: Vec::new(),
        raw: raw::Drain::default(),
    };
    // Every listener captures a weak owner and validates its socket generation on delivery.
    for name in ["open", "message", "error", "close"] {
        let weak = Rc::downgrade(owner);
        let callback = Closure::wrap(Box::new(move |event: Event| {
            if let Some(owner) = weak
                .upgrade()
                .filter(|o| o.borrow().socket.as_ref().is_some_and(|s| s.id == id))
                && let Err(error) = event_turn(&owner, name, event)
            {
                events::emit(&owner, "error", error);
                if let Some(socket) = owner.borrow_mut().socket.take() {
                    drop(socket);
                }
            }
        }) as Box<dyn FnMut(Event)>);
        // Store each closure alongside the browser callback property it serves.
        match name {
            "open" => ws.set_onopen(Some(callback.as_ref().unchecked_ref())),
            "message" => ws.set_onmessage(Some(callback.as_ref().unchecked_ref())),
            "error" => ws.set_onerror(Some(callback.as_ref().unchecked_ref())),
            _ => ws.set_onclose(Some(callback.as_ref().unchecked_ref())),
        }
        socket.listeners.push(callback);
    }
    owner.borrow_mut().socket = Some(socket);
    Ok(id)
}

/// Socket turns parse output independently of visibility and presentation.
fn event_turn(owner: &Owner, name: &str, event: Event) -> Result<(), JsValue> {
    match name {
        "open" => {
            let r = owner.borrow();
            let socket = r
                .socket
                .as_ref()
                .ok_or_else(|| value::error("TransportError", "connection disappeared"))?;
            if socket.bridge {
                control(
                    socket,
                    Client::Hello {
                        light: r
                            .session
                            .active_theme()
                            .is_some_and(|t| t.appearance == puzzle_term::theme::Appearance::Light),
                    },
                )?;
            }
            drop(r);
            flush_input(owner)?;
        }
        "message" => {
            let message = event.dyn_into::<MessageEvent>()?;
            let data = message.data();
            // Raw peers may send text; bridge peers use binary packets with checked lengths.
            let bytes = if let Some(text) = data.as_string() {
                text.into_bytes()
            } else {
                js_sys::Uint8Array::new(&data).to_vec()
            };
            let (bridge, id) = {
                let r = owner.borrow();
                let s = r
                    .socket
                    .as_ref()
                    .ok_or_else(|| value::error("TransportError", "connection disappeared"))?;
                (s.bridge, s.id)
            };
            if bridge {
                receive(owner, id, &bytes)?;
            } else {
                output::feed(owner, &bytes, None)?;
            }
        }
        "error" => {
            return Err(value::error(
                "TransportError",
                "WebSocket connection failed",
            ));
        }
        "close" => closed(owner),
        _ => {}
    }
    Ok(())
}

/// An expected drained exit is distinct from losing a running application's socket.
fn closed(owner: &Owner) {
    // The protocol exit is queued until parsing drains; either state makes closure expected.
    if owner.borrow().exit.is_none() && owner.borrow().session.exit_info().is_none() {
        events::emit(
            owner,
            "error",
            value::error("TransportError", "WebSocket connection closed"),
        );
    }
}

/// Validates each bridge message before changing input credit or terminal state.
fn receive(owner: &Owner, id: u64, bytes: &[u8]) -> Result<(), JsValue> {
    let message =
        bridge::decode_server(bytes).map_err(|e| value::error("TransportError", &e.to_string()))?;
    match message {
        // Initial grid follows the client hello; appearance was already sent before spawn.
        Server::Hello => resize(owner)?,
        // Credit is replenished by PTY writes, so buffered input advances only on these grants.
        // An initial grant authorizes sends but acknowledges no prior writer bytes.
        // Subsequent grants retire only the FIFO prefix actually sent to this peer.
        // The credit owner drops retained leases before exposing completed IDs.
        Server::InputCredit(bytes) => {
            {
                let mut r = owner.borrow_mut();
                let s = r
                    .socket
                    .as_mut()
                    .ok_or_else(|| value::error("TransportError", "connection disappeared"))?;
                s.credit
                    .grant(bytes)
                    .map_err(|e| value::error("TransportError", e))?;
                // Move scalar receipts out so route completion cannot alias socket state.
                let completions: Vec<_> =
                    std::iter::from_fn(|| s.credit.take_completed()).collect();
                // End the socket borrow before changing Session admission state.
                // Several routes may still retain a ticket, so use the route ledger.
                for id in completions {
                    r.complete_reply_route(id)
                        .map_err(|e| value::error("TransportError", &e))?;
                }
            }
            flush_input(owner)?;
            // Actual writer progress, rather than an idle polling timer, resumes output.
            // Resume an untouched parser suffix only after actual writer progress.
            // Parsing here uses the ordinary budget and never depends on visibility.
            output::feed(owner, &[], None)?;
        }
        Server::Output(bytes) => output::feed(owner, bytes, Some(id))?,
        // The wire exit follows EOF, but core parsing may still have a budgeted suffix.
        Server::Exit(code) => {
            let runtime_ms = owner.borrow().started.elapsed().as_millis() as u64;
            owner.borrow_mut().exit = Some(puzzle_term::messages::ExitInfo {
                code: Some(code),
                signal: None,
                runtime_ms,
            });
            output::finish_exit(owner)?;
        }
        Server::Error(message) => return Err(value::error("TransportError", message)),
        _ => return Err(value::error("TransportError", "unsupported bridge packet")),
    }
    Ok(())
}

/// Input is queued even before hello, then resumes only on actual server grants.
pub(super) fn write(
    owner: &Owner,
    bytes: output::WriteData,
    reply: Option<u64>,
) -> Result<bool, JsValue> {
    let mut r = owner.borrow_mut();
    let mut deferred = false;
    if let Some(s) = r.socket.as_mut() {
        if s.bridge {
            // No socket send can overtake a previous paste tail awaiting its credit.
            if let Some(id) = reply {
                match bytes {
                    output::WriteData::Owned(bytes) => s.credit.push_reply(bytes, id),
                    output::WriteData::Leased(lease) => s.credit.push_lease(Box::new(lease), id),
                }
                deferred = true;
            } else {
                s.credit.push(bytes.into_vec());
            }
        } else if s.ws.ready_state() == WebSocket::OPEN {
            s.ws.send_with_u8_array(bytes.as_slice())?;
            if let Some(id) = reply {
                s.raw.push(id, bytes.lease());
                deferred = true;
            }
        } else {
            return Err(value::error("TransportError", "raw WebSocket is not open"));
        }
    }
    drop(r);
    flush_input(owner)?;
    raw::observe(owner)?;
    Ok(deferred)
}

/// A credit event drains at most one window; it never waits for another grant.
fn flush_input(owner: &Owner) -> Result<(), JsValue> {
    let mut r = owner.borrow_mut();
    if let Some(s) = r
        .socket
        .as_mut()
        .filter(|s| s.bridge && s.ws.ready_state() == WebSocket::OPEN)
    {
        // Control messages never enter this FIFO and remain sendable with no input grant.
        while let Some(bytes) = s.credit.take_chunk() {
            control(s, Client::Input(&bytes))?;
        }
    }
    Ok(())
}

/// ACK and resize control messages do not consume input credit.
fn control(socket: &Socket, message: Client<'_>) -> Result<(), JsValue> {
    let mut bytes = Vec::new();
    bridge::encode_client(message, &mut bytes)
        .map_err(|e| value::error("TransportError", &e.to_string()))?;
    socket.ws.send_with_u8_array(&bytes)
}

/// Returns parsed output credit only to the socket incarnation that supplied it.
pub(super) fn ack(owner: &Owner, acks: Vec<(u64, u32)>) -> Result<(), JsValue> {
    let r = owner.borrow();
    for (id, bytes) in acks {
        if let Some(s) = r
            .socket
            .as_ref()
            .filter(|s| s.bridge && s.id == id && s.ws.ready_state() == WebSocket::OPEN)
        {
            control(s, Client::Ack(bytes))?;
        }
    }
    Ok(())
}

/// Every control observes the current VT grid, not dimensions from an old hello.
pub(super) fn resize(owner: &Owner) -> Result<(), JsValue> {
    let r = owner.borrow();
    let (cols, rows) = r.session.buffer().size();
    if let Some(s) = r
        .socket
        .as_ref()
        .filter(|s| s.bridge && s.ws.ready_state() == WebSocket::OPEN)
    {
        control(
            s,
            Client::Resize(bridge::Size {
                cols: cols as u16,
                rows: rows as u16,
                width_px: r.pixels[0].min(u32::from(u16::MAX)) as u16,
                height_px: r.pixels[1].min(u32::from(u16::MAX)) as u16,
            }),
        )?;
    }
    Ok(())
}
