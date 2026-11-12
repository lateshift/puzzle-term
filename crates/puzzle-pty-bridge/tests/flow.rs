//! Real cat backpressure and a position-sensitive checksum over 100 MB.
#![allow(
    clippy::unwrap_used,
    reason = "flow evidence uses assertions for all test I/O"
)]
/// Production loopback listener with test-owned temporary byte files.
#[path = "support/helpers.rs"]
mod support;
use puzzle_pty_bridge::protocol::checksum::{FNV1A_BASIS as SEED, fnv1a_update as hash};
use puzzle_pty_bridge::protocol::{self, Client, Server};
use std::{
    fs::File,
    io::Write,
    time::{Duration, Instant},
};
use support::*;
use tokio_tungstenite::tungstenite::{Error, Message, WebSocket};

/// Decimal 100 MB matches the work order rather than a smaller MiB surrogate.
const BYTES: usize = 100_000_000;
/// A periodic binary stream includes C0 bytes: raw PTY mode must preserve them.
fn corpus(path: &std::path::Path) -> u64 {
    let mut file = File::create(path).unwrap();
    let mut checksum = SEED;
    let mut buffer = vec![0; puzzle_pty_bridge::protocol::PAYLOAD_CHUNK];
    for at in (0..BYTES).step_by(buffer.len()) {
        let count = buffer.len().min(BYTES - at);
        for (index, byte) in buffer[..count].iter_mut().enumerate() {
            *byte = ((at + index) % 251) as u8;
        }
        file.write_all(&buffer[..count]).unwrap();
        checksum = hash(checksum, buffer[..count].iter().copied());
    }
    checksum
}

/// Reads until credit exhaustion without ACK; the child must still be blocked.
fn paused(socket: &mut WebSocket<std::net::TcpStream>) -> (usize, u64) {
    socket
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let mut count = 0;
    let mut checksum = SEED;
    loop {
        match socket.read() {
            Ok(Message::Binary(frame)) => {
                let Server::Output(bytes) = protocol::decode_server(&frame).unwrap() else {
                    panic!("child exited without ACK")
                };
                count += bytes.len();
                checksum = hash(checksum, bytes.iter().copied());
            }
            Err(Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return (count, checksum);
            }
            other => panic!("unexpected while withholding ACK: {other:?}"),
        }
    }
}

/// Resuming credit must deliver all remaining binary output and then exit zero.
fn resumed(
    socket: &mut WebSocket<std::net::TcpStream>,
    mut count: usize,
    mut checksum: u64,
) -> u64 {
    socket
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    send(socket, Client::Ack(count as u32));
    let mut parsed = 0;
    loop {
        let frame = receive(socket);
        match protocol::decode_server(&frame).unwrap() {
            Server::Output(bytes) => {
                count += bytes.len();
                parsed += bytes.len();
                checksum = hash(checksum, bytes.iter().copied());
                if parsed >= puzzle_pty_bridge::protocol::PAYLOAD_CHUNK {
                    send(socket, Client::Ack(parsed as u32));
                    parsed = 0;
                }
            }
            Server::Exit(code) => {
                assert_eq!(code, 0);
                assert_eq!(count, BYTES);
                return checksum;
            }
            other => panic!("unexpected after resuming: {other:?}"),
        }
    }
}

/// No ACK blocks cat around 1 MiB, while parsed-byte ACKs resume losslessly.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn no_ack_blocks_and_100mb_resumes() {
    // Info output supplies measured evidence without bypassing tracing (R10).
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .try_init();
    let temp = Temp::new();
    let path = temp.0.join("binary");
    let marker = temp.0.join("complete");
    let expected = corpus(&path);
    let host = Host::start(
        &format!(
            "stty raw -echo; cat '{}'; printf done > '{}'",
            path.display(),
            marker.display()
        ),
        None,
    );
    let mut socket = host.connect();
    hello(&mut socket, false);
    let start = Instant::now();
    let (count, checksum) = paused(&mut socket);
    assert!(
        count > puzzle_pty_bridge::protocol::OUTPUT_WINDOW
            && count
                <= puzzle_pty_bridge::protocol::OUTPUT_WINDOW
                    + puzzle_pty_bridge::protocol::PAYLOAD_CHUNK,
        "paused at {count} bytes"
    );
    assert!(
        !marker.exists(),
        "cat finished while the client withheld ACK"
    );
    assert_eq!(resumed(&mut socket, count, checksum), expected);
    assert!(marker.exists());
    tracing::info!(
        paused_bytes = count,
        transferred_bytes = BYTES,
        checksum = expected,
        elapsed_ms = start.elapsed().as_millis(),
        "bridge flow evidence"
    );
}
