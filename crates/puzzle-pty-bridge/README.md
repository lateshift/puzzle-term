# Development PTY bridge

Run `cargo xtask bridge [--shell PATH] [--port 7681] [--record FILE.vtrec] [--allow-origin ORIGIN]…`.
The warning banner and token-bearing URL appear on stdout. Lifecycle/error logs go to stderr;
`PUZZLE_LOG=info,puzzle_pty_bridge::flow=debug` enables backpressure observations. Keep the URL
private: it grants full shell access from an authorized page. This is not a production server.
It is never packaged with Puzzle Terminal; the browser demo uses its explicit connection UI.

## Security read (confirmed by Claude, 2026-10-03)

- `server.rs` binds the OS listener only to `127.0.0.1` and rechecks each accepted peer.
  There is no address option.
  Four admission permits include HTTP handshakes and sockets waiting for hello, and remain
  held through child cleanup. Excess connections receive HTTP 503.
- `security.rs` uses `getrandom::fill` for 16 bytes, serialized as 32 lowercase hex characters.
  Entropy errors prevent startup. Comparison visits all 32 bytes without an early mismatch
  return; this is source-level fixed-length comparison, not a formal timing guarantee.
  Neither tokens nor request URLs enter tracing.
- Upgrade requires exactly one UTF-8 Origin equal to an explicit allowlist member, and
  exactly `/pty?token=<token>`. Missing/duplicate metadata, extra query parameters, different
  paths and encoded credentials fail. Defaults are `http://127.0.0.1:8080` and
  `http://localhost:8080`; repeated `--allow-origin` adds exact HTTP(S) origins. This protects
  against foreign browser pages; local processes can forge HTTP headers.
  A startup warning names each added non-loopback origin: authorizing a remote page
  grants it full local shell access if it obtains the token. The warning appears even
  when `PUZZLE_LOG` suppresses tracing; it never prints the token or access URL.
- `connection.rs` limits WebSocket messages/fragments to 65,537 bytes including the tag.
  Tungstenite 0.30 caps handshake data at 64 KiB/512 reads. HTTP upgrade and hello each
  have a five-second total deadline; active socket writes have a two-second deadline.
  A successfully negotiated idle session has no timeout or periodic wakeup.
  A local process can occupy admission slots while idle. A completely silent TCP
  connection is closed by the existing five-second HTTP deadline; after HTTP upgrade,
  the first v1 hello has its own five-second deadline. Only a fully negotiated session
  can retain its slot indefinitely without sending anything further, by design.
  The fixed loopback listener limits this to local processes or explicitly authorized
  browser pages; four occupied slots deny additional connections with HTTP 503.
- `pty.rs` starts the configured login shell only after v1 hello. Its cwd is inherited HOME;
  TERM/TERMINFO, truecolor/program identity and COLORFGBG are fixed before spawn. No network
  field chooses argv or a path. Unix exit detection waits on SIGCHLD registered before
  spawn, with the original child handle retained. Disconnect uses portable-pty's safe kill/reap API
  (Unix SIGHUP with a kill fallback after its grace period), retaining the session permit.
- `flow.rs` charges reads before OS I/O. Output pauses above 1 MiB unacknowledged, with at
  most one 64 KiB overshoot, including queued and reserved reads. Forged excess ACKs fail.
  The PTY output channel and socket-sink queue each hold one chunk, plus an active
  socket write. Credit includes all these stages; the WebSocket write buffer is capped
  at two maximum messages. All output enters one FIFO ahead of exit.
- Input is ordered through one PTY writer and a preallocated one-MiB byte ring.
  The server sends `0x84` with one MiB after hello, then grants only bytes accepted
  by each actual OS write (including partial writes). The client's unspent credit
  plus queued/active input is bounded by one MiB. Packet size remains 64 KiB.
  The client retains the rest of its paste and subsequent keys in FIFO order.
  Input above granted credit produces `0x83`, closes the socket and cleans the child.
  Socket reads always remain enabled: ACK and resize apply immediately even if
  the child is not reading stdin. This breaks the echoing-child deadlock from
  the old socket-read guard. The WebSocket sink has one retained ready response,
  one queued packet and one active send; output credit includes all these stages.
- Exit follows FIFO output and PTY EOF, then the actual child status. Recording is opt-in,
  output-only, one file per session, using create-new semantics (no overwrite).

The original security read of `0c36667` was confirmed by the reviewer on 2026-10-01.
The reviewer confirmed `1ccaa28`'s event-driven coordinator, EOF ordering, original
child cleanup and zero idle turns on 2026-10-02. Its input ordering was rejected;
Claude confirmed the bidirectional-credit replacement and full bridge security/comment
read on 2026-10-03. No shell starts before HTTP authorization and the v1 hello.
`connection.rs`, `writer.rs`, `child/unix.rs` and `shutdown.rs` await readiness through
Tokio/select, with blocking workers only for native PTY I/O and final child cleanup.
Output credit wakes the blocking reader's Condvar on ACK. No recv_timeout or sleep retry
runs in an idle listener or session. Windows child-wait validation remains M13; SIGCHLD
is Unix-only and the Windows module waits on its process handle in a blocking worker.

## Shared codec and tests

The pure `puzzle-term::bridge` source is also compiled directly as this standalone binary's
`protocol` module. There is no Cargo dependency on any terminal-engine crate. The
wasm client uses `puzzle_term::bridge`; both targets compile the same tags, lengths and
little-endian conversions. Transfer diagnostics also share its streaming FNV-1a helper;
this checksum detects loss/reordering and is not an authentication mechanism.
Codec properties reject arbitrary malformed input without panic.

`cargo test -p puzzle-pty-bridge` exercises real ephemeral loopback sockets and synthetic PTY
children, never user shell startup files. Its temporary directories are cleaned up. The flow
witness withholds ACK until cat blocks, then transfers exactly 100,000,000 binary bytes and
compares a position-sensitive FNV checksum. Run with `--test flow -- --nocapture` for its
measured pause size and transfer duration. `cargo xtask accept M7` reports each bridge
criterion separately. Browser and Windows/Linux live behavior are not certified by these
macOS tests.

`--test input -- --nocapture` checks zero session/listener turns across one second
of idleness, a ten-MB raw echo through `cat` with byte-for-byte comparison, and a
64-byte paste through a real interactive shell's line editor. The non-reading
child witness fills all initial input credit while the child sleeps two seconds:
ACK and resize must apply within 500 ms, without waiting for stdin; afterwards
all 10,000,000 paste bytes plus `key` arrive in order and `stty size` is correct.
A hostile peer exceeding credit must receive `0x83`, close, and trigger the child's
SIGHUP marker. Tests use unique temporary directories and remove them on drop.
The M7.6 web client implements the same FIFO credit ledger.
Evidence: `target/m7-4-credit-{input,tests,check,accept}.log`.
