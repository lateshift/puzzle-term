//! Wake-driven bridge coordination with independent input/output credit (§12.3).
//!
//! The peer may send one MiB of input, then only bytes granted after actual
//! PTY writes. Credited input always fits the fixed byte ring without awaiting
//! capacity. Socket reads therefore remain enabled for ACK, resize and close.
//! This prevents an echoing child from deadlocking when both directions fill.
//! The sink retains bounded FIFO output, with cancellation-safe reservation
//! after retaining a ready response. Exit remains ineligible until output EOF.
//! A write completion returns byte credit, never merely a queue-capacity hint.
//! Admission remains held through original-child escalation and reaping.
use crate::{
    config::Config,
    metrics::Metrics,
    protocol::{self, Client, INPUT_WINDOW, Server},
    pty,
    record::Recorder,
    response::Pending,
    wire,
    writer::Writer,
};
use futures_util::StreamExt;
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;

/// Handles one admitted peer; all return paths finish the child's cleanup.
pub(crate) async fn run(
    stream: TcpStream,
    config: &Config,
    token: &str,
    session: u64,
    metrics: Arc<Metrics>,
) -> Result<(), String> {
    // HTTP authorization consumes only the socket. Failing here cannot
    // create a PTY, a recording file or a partially initialized child guard.
    let mut socket = wire::handshake(stream, config, token).await?;
    let light = wire::hello(&mut socket).await?;
    // Neither files nor a shell exist before authorization and the v1 hello.
    let record = config
        .record
        .as_ref()
        .map(|p| Recorder::open(p, session))
        .transpose()?;
    let mut pty = pty::spawn(config, light, record)?;
    // A slow TCP flush must not hold the socket's receive half: browser ACKs
    // unblock the output reader even while a previous output send is pending.
    let (sink, mut source) = socket.split();
    let mut writer = Writer::start(sink, pty.flow.clone());
    // Even a failed hello send takes the same off-runtime cleanup path.
    let result = async {
        writer.send(Server::Hello).await?;
        // The initial grant follows the version response in the same FIFO.
        // No later packet grants bytes merely because they entered a queue.
        writer.send(Server::InputCredit(INPUT_WINDOW)).await?;
        tracing::info!(event = "bridge.connected", session);
        pump(&mut source, &mut writer, &mut pty, &metrics).await
    }
    .await;
    if let Err(error) = &result {
        // No raw HTTP request is ever formatted as an application error.
        // Queue then flush the error before teardown, with a finite deadline.
        let _ = finish_error(&mut writer, error).await;
    }
    // Abort the sink only after the optional error flush. Session teardown
    // then drops its channels, unblocking native workers waiting to report I/O.
    drop(writer);
    // Close credit first, then perform SIGHUP/reaping off the runtime thread.
    // The admission permit stays with run's caller until this await completes.
    pty.flow.close();
    tokio::task::spawn_blocking(move || drop(pty))
        .await
        .map_err(|_| "PTY cleanup failed")?;
    result
}

/// Input completion and socket controls progress independently of PTY congestion.
async fn pump(
    source: &mut futures_util::stream::SplitStream<wire::Socket>,
    writer: &mut Writer,
    pty: &mut pty::Pty,
    metrics: &Metrics,
) -> Result<(), String> {
    // Remaining granted bytes are separate from the physical input ring:
    // an active write has left the ring but still consumes this allowance.
    let mut credit = INPUT_WINDOW;
    let mut writes_open = true;
    let mut eof = false;
    // Retention belongs to this loop, not a select future. Choosing a socket
    // event cancels only readiness waits, never an already dequeued payload.
    let mut pending = Pending::default();
    loop {
        metrics.turns.fetch_add(1, Ordering::Relaxed);
        // No socket-read guard depends on stdin capacity. The byte ledger
        // enforces memory bounds without blocking ACKs behind a large paste.
        tokio::select! {
            // Only receive into an empty pending slot; successful partial
            // writes return their exact accepted prefix, not a whole message.
            completed = pty.written.recv(), if pending.is_empty() && writes_open => {
                writes_open = pending.completed(completed)?;
            }
            // Reserve only for a ready response. Holding the sole permit
            // while awaiting input/output would starve the other producer.
            permit = writer.queue.reserve(), if !pending.is_empty() => {
                pending.send(permit.map_err(|_| "socket writer stopped")?, &mut credit)?;
            }
            message = source.next() => {
                if !receive(message, pty, &mut credit, metrics)? { return Ok(()); }
            }
            // Output preserves read order. Its EOF makes exit eligible only
            // after every earlier output has entered the common sink FIFO.
            output = pty.output.recv(), if pending.is_empty() && !eof => {
                eof = pending.output(output)?;
            }
            status = pty.child.exited(), if eof => {
                let code = status?;
                writer.send(Server::Exit(code)).await?;
                // The sink finishes only after flushing exit, which follows
                // every output frame. No try_wait or final-flush polling.
                return (&mut writer.worker).await.map_err(|_| "socket writer failed")?;
            }
            // Sink failure must wake a silent session too: otherwise its
            // child could survive indefinitely after the network disappears.
            result = &mut writer.worker => {
                return result.map_err(|_| "socket writer failed")?.and(Err("socket writer stopped".into()));
            }
        }
    }
}

/// Rejects overspending before enqueue; ACK and resize never wait for stdin.
fn receive(
    message: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
    pty: &mut pty::Pty,
    credit: &mut u32,
    metrics: &Metrics,
) -> Result<bool, String> {
    let bytes = match message {
        Some(Ok(Message::Binary(bytes))) => bytes,
        Some(Ok(Message::Close(_))) | None => return Ok(false),
        Some(Ok(Message::Ping(_) | Message::Pong(_))) => return Ok(true),
        Some(Err(error)) => return Err(wire::safe_error(error)),
        _ => return Err("bridge requires binary messages".into()),
    };
    // Native sizes and ACK counts never see malformed or truncated fields.
    match protocol::decode_client(&bytes).map_err(|e| e.to_string())? {
        Client::Input(bytes) => {
            // Debit before copying any hostile payload. Each decoded message
            // is at most 64 KiB, so its length fits u32 without truncation.
            *credit = credit
                .checked_sub(bytes.len() as u32)
                .ok_or("input exceeds granted credit")?;
            // This enqueue is synchronous and cannot wait for a non-reading
            // child: the byte ledger guarantees room for all legal input.
            pty.input.push(bytes)?;
            metrics
                .input_bytes
                .fetch_add(bytes.len() as u64, Ordering::Relaxed);
        }
        // Resize keeps the codec's device-pixel dimensions; converting them
        // to cells here would reverse the size the browser already computed.
        Client::Resize(size) => {
            pty.master
                .resize(pty::native_size(size))
                .map_err(|e| e.to_string())?;
            metrics.resizes.fetch_add(1, Ordering::Relaxed);
        }
        Client::Ack(bytes) => {
            // Forged future credit is rejected by Flow. A valid ACK wakes
            // the blocked PTY output reader without waiting for stdin writes.
            pty.flow.ack(bytes)?;
            metrics.acks.fetch_add(1, Ordering::Relaxed);
        }
        Client::Hello { .. } => return Err("duplicate hello".into()),
    }
    Ok(true)
}

/// Flushes an error without creating a timer on any successful idle session.
async fn finish_error(writer: &mut Writer, error: &str) -> Result<(), String> {
    // A failed sink has already completed in select; never poll its completed
    // JoinHandle again or try to flush through that disconnected writer.
    if writer.worker.is_finished() {
        return Ok(());
    }
    // An error packet is followed by shutdown of the queue. The writer then
    // drains and returns, giving malformed-message tests a visible response.
    tokio::time::timeout(Duration::from_secs(2), async {
        writer.send(Server::Error(error)).await?;
        let (closed, _) = tokio::sync::mpsc::channel(1);
        writer.queue = closed;
        (&mut writer.worker)
            .await
            .map_err(|_| "socket writer failed")?
    })
    .await
    .map_err(|_| "WebSocket error flush timeout".to_string())?
}
