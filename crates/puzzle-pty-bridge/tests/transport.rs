//! Real PTY output, input, size and recording pass through production sockets.
#![allow(
    clippy::unwrap_used,
    reason = "literal bridge contracts fail directly in tests"
)]
/// Loopback host and private temporary files shared by socket witnesses.
#[path = "support/helpers.rs"]
mod support;
use puzzle_pty_bridge::protocol::{Client, Size};
use support::*;

/// PTY EOF must drain every final byte before sending the actual child status.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn output_before_exit() {
    let host = Host::start("printf done; exit 3", None);
    let mut socket = host.connect();
    hello(&mut socket, false);
    let (bytes, code) = drain(&mut socket);
    assert_eq!(bytes, b"done");
    assert_eq!(code, 3);
}

/// Both appearances are fixed before spawn; ncurses sees our bundled Sync entry.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn identity_and_sync() {
    let command = "printf '%s|%s|%s|%s|' \"$TERM\" \"$COLORTERM\" \"$TERM_PROGRAM\" \"$COLORFGBG\"; tput Sync 1";
    let host = Host::start(command, None);
    for light in [false, true] {
        let mut socket = host.connect();
        hello(&mut socket, light);
        let (bytes, code) = drain(&mut socket);
        assert_eq!(code, 0, "tput Sync failed");
        let hint = if light { "0;15" } else { "15;0" };
        assert_eq!(
            bytes,
            format!("xterm-puzzle|truecolor|puzzle|{hint}|\x1b[?2026h").as_bytes()
        );
    }
}

/// Resize is ordered before input, with wire axes preserved in stty's reply.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn input_and_resize() {
    let host = Host::start(
        "stty -echo; printf ready; read line; stty size; printf 'input:%s' \"$line\"",
        None,
    );
    let mut socket = host.connect();
    hello(&mut socket, false);
    assert_eq!(receive(&mut socket), b"\x80ready");
    send(
        &mut socket,
        Client::Resize(Size {
            cols: 120,
            rows: 40,
            width_px: 960,
            height_px: 640,
        }),
    );
    send(&mut socket, Client::Input(b"hello\r"));
    let (bytes, code) = drain(&mut socket);
    assert_eq!(code, 0);
    assert_eq!(bytes, b"40 120\r\ninput:hello");
}

/// The .vtrec header is LE delay/length, and concatenating reads preserves output.
#[test]
#[ignore = "D-46/U2: real-PTY/socket witness, runs in accept closure"]
fn recording_raw_output() {
    let temp = Temp::new();
    let path = temp.0.join("session.vtrec");
    let host = Host::start(
        "printf first; sleep 0.05; printf second",
        Some(path.clone()),
    );
    let mut socket = host.connect();
    hello(&mut socket, false);
    let (output, code) = drain(&mut socket);
    assert_eq!((output.as_slice(), code), (b"firstsecond".as_slice(), 0));
    let recording = std::fs::read(path).unwrap();
    let mut rest = recording.as_slice();
    let mut bytes = Vec::new();
    let mut delays = 0_u64;
    while !rest.is_empty() {
        assert!(rest.len() >= 8);
        delays += u64::from(u32::from_le_bytes(rest[..4].try_into().unwrap()));
        let len = u32::from_le_bytes(rest[4..8].try_into().unwrap()) as usize;
        assert!(len > 0 && len <= puzzle_pty_bridge::protocol::PAYLOAD_CHUNK);
        bytes.extend_from_slice(&rest[8..8 + len]);
        rest = &rest[8 + len..];
    }
    assert_eq!(bytes, output);
    assert!(delays >= 30, "recording lost the deliberate child delay");
}
