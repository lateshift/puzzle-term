//! Bidirectional credit over real sockets: an echoing child must never deadlock.
#![allow(
    clippy::unwrap_used,
    reason = "real byte-integrity assertions fail directly"
)]
/// Credit-aware test client, independent of the production codec's accounting.
#[path = "support/credited.rs"]
mod credited;
/// Neutral shell commands and unique temporary evidence, never user recordings.
#[path = "support/helpers.rs"]
mod support;
use credited::transfer;
use puzzle_pty_bridge::protocol::{Client, Server};
use std::time::{Duration, Instant};
use support::*;

/// cat reads raw input and writes the same bytes; both credit ledgers fill.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn echoing_10mb_paste_completes_in_order() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let host = Host::start("stty raw -echo; printf ready; exec cat", None);
    let mut socket = host.connect();
    let credit = hello(&mut socket, false);
    assert_eq!(receive(&mut socket), b"\x80ready");
    send(&mut socket, Client::Ack(5));
    let paste: Vec<u8> = (0..10_000_000).map(|at| (at % 251) as u8).collect();
    let started = Instant::now();
    let output = transfer(&mut socket, &paste, credit, paste.len());
    assert_eq!(output, paste);
    assert!(started.elapsed() < Duration::from_secs(10));
    tracing::info!(
        bytes = paste.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "echoing paste exact"
    );
}

/// A real interactive shell's canonical line editor echoes a modest paste.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn interactive_shell_line_paste_completes() {
    let host = Host::start(
        "stty sane echo; PS1='ready'; export PS1; exec /bin/sh -i",
        None,
    );
    let mut socket = host.connect();
    let credit = hello(&mut socket, false);
    let mut prompt = Vec::new();
    while !prompt.ends_with(b"ready") {
        let bytes = receive(&mut socket);
        assert_eq!(bytes[0], 0x80);
        prompt.extend_from_slice(&bytes[1..]);
        send(&mut socket, Client::Ack((bytes.len() - 1) as u32));
    }
    // Below the prompt plus 80-column wrap boundary: the shell line editor
    // should echo literal text rather than emit screen-redraw control bytes.
    let paste: Vec<u8> = (0..64).map(|at| b'a' + (at % 26) as u8).collect();
    assert_eq!(transfer(&mut socket, &paste, credit, paste.len()), paste);
    send(&mut socket, Client::Input(b"\x15exit\n"));
    assert_eq!(drain(&mut socket).1, 0);
}

/// ACK and resize apply while all initial credit sits behind an unread PTY.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn unread_10mb_input_backpressures_without_disconnect() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let temp = Temp::new();
    let path = temp.0.join("input.bin");
    let host = Host::start(
        &format!(
            "stty raw -echo; printf ready; sleep 2; head -c 10000003 > '{}'; stty size; printf complete; read tail",
            path.display()
        ),
        None,
    );
    let mut socket = host.connect();
    let mut credit = hello(&mut socket, false);
    assert_eq!(receive(&mut socket), b"\x80ready");
    let mut paste: Vec<u8> = (0..10_000_000).map(|at| (at % 251) as u8).collect();
    paste.extend_from_slice(b"key");
    for chunk in paste[..credit as usize].chunks(puzzle_pty_bridge::protocol::PAYLOAD_CHUNK) {
        send(&mut socket, Client::Input(chunk));
    }
    let sent = credit as usize;
    credit = 0;
    let control_ms = credited::stalled_controls(&mut socket, &host.metrics, sent);
    let started = Instant::now();
    let output = transfer(
        &mut socket,
        &paste[sent..],
        credit,
        b"40 120\ncomplete".len(),
    );
    assert_eq!(output, b"40 120\ncomplete");
    // M16.6a/U2: retain the original M7.4 byte-for-byte paste→key order check.
    // A matching completion marker alone cannot prove that stdin stayed ordered.
    assert_eq!(
        std::fs::read(path).unwrap(),
        paste,
        "M7.4 exact input order: all 10,000,000 paste bytes, then key"
    );
    tracing::info!(
        bytes = paste.len(),
        control_ms,
        elapsed_ms = started.elapsed().as_millis(),
        "unread paste and key exact"
    );
    send(&mut socket, Client::Input(b"done\n"));
    assert_eq!(drain(&mut socket).1, 0);
}

/// Excess input is rejected, and original-child teardown still signals/reaps.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn overspending_credit_errors_and_cleans_child() {
    let temp = Temp::new();
    let marker = temp.0.join("hup");
    let host = Host::start(
        &format!(
            "trap 'printf hup > {}; exit' HUP; stty raw -echo; printf ready; sleep 30 & wait",
            marker.display()
        ),
        None,
    );
    let mut socket = host.connect();
    let credit = hello(&mut socket, false);
    assert_eq!(receive(&mut socket), b"\x80ready");
    // Do not consume replenishment packets: the OS may accept a prefix, but
    // overspending two complete windows exceeds even that tiny PTY buffer.
    for _ in 0..(credit as usize * 2 / puzzle_pty_bridge::protocol::PAYLOAD_CHUNK) {
        let mut bytes = Vec::new();
        puzzle_pty_bridge::protocol::encode_client(
            Client::Input(&[b'x'; puzzle_pty_bridge::protocol::PAYLOAD_CHUNK]),
            &mut bytes,
        )
        .unwrap();
        // Rejection can race the hostile sender. Stop writing on closure,
        // then inspect the already-flushed error rather than unwrap BrokenPipe.
        if socket
            .send(tokio_tungstenite::tungstenite::Message::Binary(
                bytes.into(),
            ))
            .is_err()
        {
            break;
        }
    }
    loop {
        let bytes = receive(&mut socket);
        if let Server::Error(reason) = puzzle_pty_bridge::protocol::decode_server(&bytes).unwrap() {
            assert_eq!(reason, "input exceeds granted credit");
            break;
        }
    }
    assert!(socket.read().is_err(), "error must close the session");
    let deadline = Instant::now() + Duration::from_secs(3);
    while !marker.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(std::fs::read(marker).unwrap(), b"hup");
}

/// A quiet shell has no timer, credit heartbeat or polling accept loop.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn idle_session_has_zero_periodic_turns() {
    let host = Host::start("stty -echo; read answer; printf awake", None);
    let mut socket = host.connect();
    hello(&mut socket, false);
    let deadline = Instant::now() + Duration::from_secs(1);
    while host.metrics.session_turns() == 0 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert_eq!(host.metrics.session_turns(), 1);
    let sessions = host.metrics.session_turns();
    let admissions = host.metrics.admission_turns();
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(host.metrics.session_turns() - sessions, 0);
    assert_eq!(host.metrics.admission_turns() - admissions, 0);
    send(&mut socket, Client::Input(b"done\n"));
    assert_eq!(drain(&mut socket), (b"awake".to_vec(), 0));
}
