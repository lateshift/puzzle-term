//! One async WebSocket sink preserves output/exit order under TCP backpressure.
//!
//! Splitting the socket lets ACKs and close arrive during a slow flush. The
//! queue has one message plus one active write; no send task is spawned per
//! chunk. Each active write gets a finite two-second timeout, but idleness
//! only awaits the channel and has no timer.
//!
//! The socket task alone owns the sink: output, hello, errors and exit cannot
//! acquire independent send futures and reorder. The coordinator owns the
//! read half, so an ACK can return credit while a TCP flush is in progress.
//! The coordinator retains at most one pending response outside this channel.
//! It reserves only after that response is ready, so neither an idle output
//! receiver nor an idle credit receiver can monopolize the single sink slot.
//! This queue is distinct from the credit bound: the latter includes bytes
//! already flushed to the peer but not yet acknowledged by its parser.
//! Both bounds are needed to protect memory when either endpoint is slow.
use crate::{
    flow::Flow,
    protocol::{self, Server},
    wire::{self, Socket},
};
use futures_util::{SinkExt, stream::SplitSink};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// An encoded frame and the output bytes eligible for the next parsed-byte ACK.
pub(crate) struct Packet {
    /// WebSocket message owns the bytes throughout a partial TCP flush.
    message: Message,
    /// Only output, not tags/hello/errors, consumes the PTY credit ledger.
    credit: usize,
}

/// Bounded socket task plus the sender the session selects for readiness.
pub(crate) struct Writer {
    /// Capacity one: waiting here cannot grow a hidden output backlog.
    pub queue: mpsc::Sender<Packet>,
    /// A failed sink wakes the coordinator even if the child is silent.
    pub worker: tokio::task::JoinHandle<Result<(), String>>,
}

/// A single sink serializes all messages; EOF uses the same queue as output.
impl Writer {
    /// Starts one socket writer, not a blocking-pool job or a periodic timer.
    pub(crate) fn start(sink: SplitSink<Socket, Message>, flow: Arc<Flow>) -> Self {
        let (queue, incoming) = mpsc::channel(1);
        Self {
            queue,
            worker: tokio::spawn(write(sink, incoming, flow)),
        }
    }
    /// Enqueues a bounded application response; socket errors remain observable.
    pub(crate) async fn send(&self, message: Server<'_>) -> Result<(), String> {
        self.queue
            .send(packet(message)?)
            .await
            .map_err(|_| "socket writer stopped".into())
    }
}

/// A coordinator error or cancellation cannot leave a detached socket writer.
impl Drop for Writer {
    /// Abort only discards a connection already being torn down, never live input.
    fn drop(&mut self) {
        self.worker.abort();
    }
}

/// Encodes at the native boundary, retaining raw terminal bytes unchanged.
pub(crate) fn packet(message: Server<'_>) -> Result<Packet, String> {
    let credit = if let Server::Output(bytes) = &message {
        bytes.len()
    } else {
        0
    };
    let mut bytes = Vec::new();
    protocol::encode_server(message, &mut bytes).map_err(|e| e.to_string())?;
    Ok(Packet {
        message: Message::Binary(bytes.into()),
        credit,
    })
}

/// Sleeps on an empty queue and socket readiness, with deadlines only for writes.
async fn write(
    mut sink: SplitSink<Socket, Message>,
    mut incoming: mpsc::Receiver<Packet>,
    flow: Arc<Flow>,
) -> Result<(), String> {
    while let Some(packet) = incoming.recv().await {
        // Register credit before exposing output to the peer. An ACK racing
        // the completed send must never be mistaken for forged future credit.
        flow.sent(packet.credit)?;
        let final_message =
            matches!(&packet.message, Message::Binary(bytes) if bytes.first() == Some(&0x81));
        tokio::time::timeout(Duration::from_secs(2), sink.send(packet.message))
            .await
            .map_err(|_| "WebSocket write timeout".to_string())?
            .map_err(wire::safe_error)?;
        if final_message {
            return Ok(());
        }
    }
    Ok(())
}
