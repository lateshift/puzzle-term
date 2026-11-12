//! Minimal peer-side credit driver: output ACKs progress while paste is queued.
use super::support::{receive, send};
use puzzle_pty_bridge::protocol::{self, Client, Server};
use std::{
    net::TcpStream,
    time::{Duration, Instant},
};
use tokio_tungstenite::tungstenite::WebSocket;

/// Sends only granted bytes, ACKs every output chunk, and retains FIFO input.
pub(crate) fn transfer(
    socket: &mut WebSocket<TcpStream>,
    input: &[u8],
    mut credit: u32,
    expected: usize,
) -> Vec<u8> {
    let mut sent = 0;
    let mut output = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while sent < input.len() || output.len() < expected {
        assert!(
            Instant::now() < deadline,
            "paste stalled: sent {sent}, output {}",
            output.len()
        );
        while credit > 0 && sent < input.len() {
            let count = (credit as usize)
                .min(puzzle_pty_bridge::protocol::PAYLOAD_CHUNK)
                .min(input.len() - sent);
            send(socket, Client::Input(&input[sent..sent + count]));
            credit -= count as u32;
            sent += count;
        }
        let bytes = receive(socket);
        match protocol::decode_server(&bytes).unwrap() {
            Server::InputCredit(bytes) => credit += bytes,
            Server::Output(bytes) => {
                output.extend_from_slice(bytes);
                send(socket, Client::Ack(bytes.len() as u32));
            }
            other => panic!("paste ended prematurely: {other:?}"),
        }
    }
    output
}

/// Witnesses control progress before the non-reading child's two-second sleep ends.
pub(crate) fn stalled_controls(
    socket: &mut WebSocket<TcpStream>,
    metrics: &puzzle_pty_bridge::metrics::Metrics,
    sent: usize,
) -> f64 {
    send(
        socket,
        Client::Resize(protocol::Size {
            cols: 120,
            rows: 40,
            width_px: 960,
            height_px: 640,
        }),
    );
    send(socket, Client::Ack(5));
    let started = Instant::now();
    let deadline = started + Duration::from_millis(500);
    // Server counters are incremented after each OS resize / credit update,
    // not merely when the request enters the client's TCP send buffer.
    while (metrics.resizes() == 0 || metrics.acks() == 0 || metrics.input_bytes() < sent as u64)
        && Instant::now() < deadline
    {
        std::thread::yield_now();
    }
    assert_eq!(metrics.resizes(), 1, "resize stuck behind input");
    assert_eq!(metrics.acks(), 1, "ACK stuck behind input");
    assert_eq!(metrics.input_bytes(), sent as u64);
    started.elapsed().as_secs_f64() * 1000.0
}
