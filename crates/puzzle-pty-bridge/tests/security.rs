//! Authentication and capacity are tested through the real HTTP upgrade.
#![allow(
    clippy::unwrap_used,
    reason = "assertions expose setup or contract failures in tests"
)]
/// Real clients and temporary synthetic evidence.
#[path = "support/helpers.rs"]
mod support;
use puzzle_pty_bridge::protocol::Client;
use std::time::{Duration, Instant};
use support::*;

/// Absent credentials must never be accepted even from an allowed page.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn missing_token_rejected() {
    let host = Host::start("sleep 30", None);
    rejected(
        connect(
            &format!("ws://{}/pty", host.address),
            "http://localhost:8080",
        ),
        403,
    );
}

/// A same-length wrong token cannot reuse a nearby matching prefix.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn wrong_token_rejected() {
    let host = Host::start("sleep 30", None);
    let mut url = host.url.clone();
    let last = url.pop().unwrap();
    url.push(if last == '0' { '1' } else { '0' });
    rejected(connect(&url, "http://localhost:8080"), 403);
}

/// Knowing the token is insufficient from an unapproved browser Origin.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn bad_origin_rejected() {
    let host = Host::start("sleep 30", None);
    for origin in [
        "null",
        "http://evil.test",
        "http://localhost:8080.evil.test",
        "http://localhost:8080/",
    ] {
        rejected(connect(&host.url, origin), 403);
    }
}

/// Four pre-hello sessions hold slots too, and none has created a shell yet.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn fifth_session_and_prehello() {
    let temp = Temp::new();
    let marker = temp.0.join("started");
    let host = Host::start(
        &format!("printf started > '{}'; sleep 30", marker.display()),
        None,
    );
    let mut clients: Vec<_> = (0..4).map(|_| host.connect()).collect();
    rejected(connect(&host.url, "http://localhost:8080"), 503);
    std::thread::sleep(Duration::from_millis(50));
    assert!(!marker.exists(), "HTTP upgrade alone spawned a shell");
    hello(&mut clients[0], false);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !marker.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(marker.exists());
    for socket in clients {
        socket.get_ref().shutdown(std::net::Shutdown::Both).unwrap();
    }
}

/// The listener's actual OS address cannot be wildcard, IPv6-any or a LAN IP.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn listener_is_loopback() {
    let host = Host::start("exit 0", None);
    assert_eq!(host.address.ip(), std::net::Ipv4Addr::LOCALHOST);
}

/// Malformed first frames and duplicate hellos fail before any new child work.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn malformed_protocol_closes() {
    let host = Host::start("sleep 30", None);
    for bytes in [vec![0, 4], vec![3, 2, 0], vec![3, 1, 2], vec![1]] {
        let mut socket = host.connect();
        socket
            .send(tokio_tungstenite::tungstenite::Message::Binary(
                bytes.into(),
            ))
            .unwrap();
        assert!(socket.read().is_err());
    }
    let mut socket = host.connect();
    hello(&mut socket, false);
    send(&mut socket, Client::Hello { light: false });
    assert_eq!(receive(&mut socket)[0], 0x83);
    let mut socket = host.connect();
    hello(&mut socket, false);
    send(&mut socket, Client::Ack(u32::MAX));
    assert_eq!(receive(&mut socket)[0], 0x83);
}

/// Disconnect signals the child with SIGHUP even while no output is pending.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn disconnect_sends_sighup() {
    let temp = Temp::new();
    let marker = temp.0.join("hup");
    let host = Host::start(
        &format!(
            "trap 'printf hup > {}' HUP; printf ready; read answer",
            marker.display()
        ),
        None,
    );
    let mut socket = host.connect();
    hello(&mut socket, false);
    assert_eq!(receive(&mut socket), b"\x80ready");
    socket.get_ref().shutdown(std::net::Shutdown::Both).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !marker.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(std::fs::read(&marker).unwrap(), b"hup");
}

/// Both default development origins succeed, ruling out an always-reject server.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn default_origins_succeed() {
    let host = Host::start("printf ok", None);
    for origin in ["http://localhost:8080", "http://127.0.0.1:8080"] {
        let mut socket = connect(&host.url, origin).unwrap();
        hello(&mut socket, false);
        assert_eq!(drain(&mut socket), (b"ok".to_vec(), 0));
    }
}
