//! Single-owner regression at initial and steady sparse scrollback capacity.
//! The input fills more than three complete 32-MiB histories. It runs through
//! the real fixed pool and parser worker, so both cold row allocation and the
//! first recycle boundary remain in the retained feed evidence. A trailing mark
//! on every row makes historical sparse accounting part of the workload.
use super::*;
use crate::pty_pool;
use puzzle_term::{
    SessionConfig,
    platform::{PtySize, Services, Transport, Waker},
};
use std::io::Cursor;

/// This finite output-only corpus contains no application queries or input.
struct NullTransport;
impl Transport for NullTransport {
    /// Replies are absent; accepting the interface keeps the real Session path.
    fn write(&mut self, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    /// Capacity is reached without changing the terminal's initial geometry.
    fn resize(&mut self, _: PtySize) -> Result<(), String> {
        Ok(())
    }
    /// EOF comes from the finite pool producer rather than a child process.
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// Output-only tests require neither a native event loop nor a rendering task.
struct NoFrame;
impl Waker for NoFrame {
    /// Damage remains observable in Session; no host frame is requested here.
    fn request_frame(&mut self) {}
    /// The regression supplies all output eagerly and owns no animation timer.
    fn wake_at(&mut self, _: Instant) {}
}

/// Source construction occurs before the owner starts feeding the fixed pool.
fn fixture() -> Vec<u8> {
    let row = format!("{}\u{0301}\r\n", "漢".repeat(141));
    // Three raw-cell history capacities also exceed the smaller sparse-byte cap.
    row.repeat(3 * (32 * 1024 * 1024 / (282 * 16) + 63))
        .into_bytes()
}

/// Sparse history recycles within the byte cap without relocating its row deque.
#[test]
#[ignore = "explicit release observations across a real storage capacity boundary"]
fn sparse_history_capacity_preserves_bounds() -> Result<(), String> {
    let bytes = fixture();
    let expected = bytes.len() as u64;
    let (mut producer, consumer) = pty_pool::pair();
    let session = Session::new(
        SessionConfig {
            columns: 282,
            rows: 63,
            ..SessionConfig::default()
        },
        Services::new(Box::new(NullTransport), Box::new(NoFrame)),
    );
    let started = start(session, consumer, Config::default())?;
    let mut source = Cursor::new(bytes);
    while producer
        .read_once(&mut source)
        .map_err(|error| error.to_string())?
    {}
    let deadline = Instant::now() + Duration::from_secs(10);
    while started.handle.parsed_total() < expected && Instant::now() < deadline {
        thread::yield_now();
    }
    assert_eq!(started.handle.parsed_total(), expected);
    started.handle.probe(deadline, |session| {
        let ring = &session.terminal().primary.ring;
        assert!(ring.scrollback_len() > 1000);
        assert!(ring.history_bytes() <= 32 * 1024 * 1024);
        assert_eq!(ring.storage_growth().deque_growths, 0);
        assert!(
            ring.storage_growth().rows < 8000,
            "recycling at the byte cap allocated new rows"
        );
    })?;
    drop(producer);
    started.worker.join().map_err(|_| "owner panicked")??;
    let report = started.handle.stage_report();
    assert!(
        report["max_feed_operations"]["cluster_joins"]
            .as_u64()
            .is_some_and(|count| count > 0),
        "covered CJK/combining feeds supplied no semantic operation measurements"
    );
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/m16-5");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    std::fs::write(
        directory.join("history-capacity.json"),
        serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    // D-50 withdraws the shared-lock ceiling, not the byte/storage invariants.
    // Paired owner feed clocks above remain evidence for end-to-end acceptance.
    Ok(())
}
