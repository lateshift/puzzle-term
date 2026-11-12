//! Actual PTY write blocking exercises credit, priority controls and joined teardown.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "real-PTY proofs name each failed precondition"
)]
use super::*;
use crate::{pipeline::Pipeline, pty, pty_pool, window::live::start_reader};
use portable_pty::CommandBuilder;
use puzzle_term::platform::reply::PENDING_BYTE_ALLOWANCE;
use std::{fs, path::PathBuf};

/// Generated test artifacts contain only query receipts, never shell/user output.
struct Fixture {
    /// Drop cancels and joins every worker, including on an assertion failure.
    pipeline: Option<Pipeline>,
    /// D-50 priority input remains available while the writer refuses new bytes.
    handle: Handle,
    /// Frame replies have a separate inbox even while program replies are blocked.
    frames: Frames,
    /// Producer reads stdin only after the test creates this file.
    gate: PathBuf,
    /// Atomic child summary contains exact byte count and key position.
    result: PathBuf,
    /// Child's total query count is derived from the existing byte allowance.
    queries: usize,
    /// All artifacts are removed after joined child shutdown.
    directory: PathBuf,
}
impl Fixture {
    /// Starts the product drain/writer/owner without creating a GPU or native window.
    fn new() -> Self {
        let directory = directory();
        let gate = directory.join("read");
        let result = directory.join("result.json");
        let queries = 2 * PENDING_BYTE_ALLOWANCE / b"\x1b[?62;22c".len();
        let mut command = CommandBuilder::new("python3");
        command.args(["-u", "-c", CHILD]);
        command.arg(&gate);
        command.arg(&result);
        command.arg(queries.to_string());
        let mut child = pty::spawn(
            command,
            PtySize {
                columns: 80,
                rows: 24,
                pixel_width: 800,
                pixel_height: 480,
            },
        )
        .unwrap();
        let killer = child.child.clone_killer();
        let writer = child.transport.start_writer().unwrap();
        let (producer, consumer) = pty_pool::pair();
        let (send_exit, exit) = mpsc::channel();
        let drain = start_reader(
            child.reader,
            producer,
            child.child,
            child.started,
            send_exit,
        );
        let mut pipeline = Pipeline::new(killer, writer, drain);
        let session = Session::new(
            SessionConfig::default(),
            Services::new(Box::new(child.transport), Box::new(Quiet)),
        );
        let started = start(
            session,
            consumer,
            Config {
                exit: Some(exit),
                ..Config::default()
            },
        )
        .unwrap();
        pipeline.attach(started.handle.clone(), started.worker);
        Self {
            pipeline: Some(pipeline),
            handle: started.handle,
            frames: started.frames,
            gate,
            result,
            queries,
            directory,
        }
    }

    /// Waits for actual backpressure, not a guessed sleep or a speed threshold.
    fn blocked(&self, end: Instant) -> usize {
        loop {
            let pending = self
                .handle
                .probe(end, |session| session.pending_reply_bytes())
                .unwrap();
            assert!(
                pending <= PENDING_BYTE_ALLOWANCE,
                "reply queue exceeded its byte allowance"
            );
            if pending >= PENDING_BYTE_ALLOWANCE - 2 * puzzle_term::vt::osc::MAX_BUFFER_BYTES {
                return pending;
            }
            assert!(
                Instant::now() < end,
                "child never reached reply backpressure"
            );
            thread::yield_now();
        }
    }
}
/// Private paths keep parallel real-PTY fixtures independent of one another.
fn directory() -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "puzzle-reply-flood-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    directory
}
impl Drop for Fixture {
    /// Pipeline's drop runs first through explicit cancellation before deleting artifacts.
    fn drop(&mut self) {
        if let Some(pipeline) = self.pipeline.as_mut() {
            pipeline.cancel();
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}

/// The child cannot read, yet owner-side key encoding and a subsequent probe complete.
/// Once it reads again, every reply is exact and the key retains its original position.
#[test]
fn nonreading_real_pty_reply_flood_is_bounded_ordered_and_resumes() {
    let fixture = Fixture::new();
    let end = Instant::now() + crate::test_support::HANG_GUARD;
    let high = fixture.blocked(end);
    crate::test_support::report(format_args!(
        "M19_REPLY pending_bytes={high} allowance={PENDING_BYTE_ALLOWANCE}"
    ));
    // Headless publication still answers through its independent control/slot lane.
    let id = fixture
        .handle
        .request_frame(Instant::now(), 0, 0, end)
        .unwrap();
    assert_eq!(
        fixture.frames.receive_until(end).expect("frame reply").id,
        id
    );
    let before_key = fixture
        .handle
        .probe(end, |session| {
            session
                .apply(
                    SessionMessage::Command(Command::Text("K".into()), Origin::User),
                    Instant::now(),
                )
                .unwrap();
            session.parsed_bytes_total()
        })
        .unwrap() as usize;
    fs::write(&fixture.gate, b"read").unwrap();
    while !fixture.result.is_file() {
        let pending = fixture
            .handle
            .probe(end, |session| session.pending_reply_bytes())
            .unwrap();
        assert!(pending <= PENDING_BYTE_ALLOWANCE);
        assert!(Instant::now() < end, "reply flood failed to resume");
        thread::yield_now();
    }
    let result: serde_json::Value =
        serde_json::from_slice(&fs::read(&fixture.result).unwrap()).unwrap();
    assert_eq!(result["bytes"], fixture.queries * b"\x1b[?62;22c".len() + 1);
    assert_eq!(
        result["key"],
        before_key / b"\x1b[c".len() * b"\x1b[?62;22c".len()
    );
    assert_eq!(result["valid"], true, "a reply was corrupted or discarded");
    crate::test_support::report(format_args!(
        "M19_REPLY exact_bytes={} key_offset={} queries={}",
        result["bytes"], result["key"], fixture.queries
    ));
}

/// Closing the same stalled lane still joins the writer, drain and Session owner.
#[test]
fn close_during_nonreading_reply_flood_joins_and_reaps() {
    let mut fixture = Fixture::new();
    fixture.blocked(Instant::now() + crate::test_support::HANG_GUARD);
    let pipeline = fixture.pipeline.take().unwrap();
    let pid = pipeline.drain_pid();
    let handle = fixture.handle.clone();
    let (done, joined) = mpsc::channel();
    let joiner = thread::spawn(move || {
        drop(pipeline);
        done.send(()).unwrap();
    });
    joined
        .recv_timeout(crate::test_support::HANG_GUARD)
        .expect("close deadlocked behind reply credit");
    joiner.join().unwrap();
    assert!(
        handle
            .send(SessionMessage::Command(
                Command::Text("late".into()),
                Origin::User
            ))
            .is_err()
    );
    let process = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "stat="])
        .output()
        .unwrap();
    assert!(process.stderr.is_empty(), "process inspection failed");
    assert!(
        process.stdout.iter().all(u8::is_ascii_whitespace),
        "child remained after joined close"
    );
}

/// Test-only coordination polling does no reading before the parent grants it.
/// All bytes after that point are checked against the ordinary DA1 response.
const CHILD: &str = r#"
import os, sys, threading, time, tty, json
tty.setraw(0)
gate, result, count = sys.argv[1], sys.argv[2], int(sys.argv[3])
def consume():
    while not os.path.exists(gate): time.sleep(0.01)
    expected = count * len(b'\x1b[?62;22c') + 1
    received = bytearray()
    while len(received) < expected:
        received.extend(os.read(0, min(65536, expected - len(received))))
    key = received.find(b'K')
    plain = bytes(received[:key] + received[key+1:]) if key >= 0 else bytes(received)
    data = {'bytes': len(received), 'key': key, 'valid': plain == b'\x1b[?62;22c' * count and received.count(b'K') == 1}
    with open(result + '.tmp', 'w') as file: json.dump(data, file)
    os.replace(result + '.tmp', result)
reader = threading.Thread(target=consume)
reader.start()
source = b'\x1b[c' * count
while source:
    written = os.write(1, source)
    source = source[written:]
reader.join()
"#;
