//! Headless real-PTY stage measurements isolate parsing from the renderer.
//! Run explicitly in release with PUZZLE_UNICODE_CORPUS and --ignored. The test
//! keeps every trial, measures with and without refresh-rate publication requests,
//! and verifies exact bytes through the production fixed pool and parse worker.
use super::*;
use crate::{pty, pty_pool};
use portable_pty::CommandBuilder;
use puzzle_term::{
    SessionConfig,
    platform::{PtySize, Services, Waker},
};
use std::{path::PathBuf, time::Duration};

/// An actual output-only child has no queries; input transport remains intact.
struct NoFrame;
impl Waker for NoFrame {
    /// This headless stage deliberately has no renderer or window wakeups.
    fn request_frame(&mut self) {}
    /// No animation timer drives the manually requested snapshots.
    fn wake_at(&mut self, _: Instant) {}
}

/// Constructs the output child and geometry outside the measured owner interval.
fn stage_owner(path: &PathBuf) -> Result<(Started, thread::JoinHandle<()>, u64, Instant), String> {
    let mut command = CommandBuilder::new("/bin/sh");
    command.args([
        "-c",
        "stty raw -echo; printf '\\033[?1049h'; cat \"$PUZZLE_STAGE_CORPUS\"",
    ]);
    command.env("PUZZLE_STAGE_CORPUS", path);
    let spawned = pty::spawn(
        command,
        PtySize {
            columns: 282,
            rows: 63,
            pixel_width: 2820,
            pixel_height: 1260,
        },
    )?;
    let (mut producer, consumer) = pty_pool::pair();
    let mut session = Session::new(
        SessionConfig {
            columns: 282,
            rows: 63,
            ..SessionConfig::default()
        },
        Services::new(Box::new(spawned.transport), Box::new(NoFrame)),
    );
    let context = puzzle_term::frame_builder::context::TextContext::new(
        crate::font_data::bundled_font_set()?,
        14.0,
        1.0,
    )?;
    session.set_text_context(&context);
    let started = Instant::now();
    let owner = start(session, consumer, Config::default())?;
    let drain = thread::spawn(move || {
        let mut reader = spawned.reader;
        while producer.read_once(reader.as_mut()).is_ok_and(|read| read) {}
    });
    Ok((owner, drain, context.atlas_generation(), started))
}

/// Measures one chronological trial without counting the polling interval as parsing.
fn trial(path: &PathBuf, expected: u64, snapshots: bool) -> Result<serde_json::Value, String> {
    let (owner, drain, generation, started) = stage_owner(path)?;
    let mut publications = Vec::new();
    while !owner.worker.is_finished() {
        if started.elapsed() > Duration::from_secs(10) {
            return Err("PTY stage timed out".into());
        }
        if snapshots {
            let now = Instant::now();
            let deadline = now + Duration::from_nanos(16_666_667);
            owner.handle.request_frame(now, generation, 0, deadline)?;
            if let Some(reply) = owner.frames.receive_until(deadline) {
                publications.push(reply.timing.report());
                owner.frames.recycle(reply.slot);
            }
        }
        thread::sleep(Duration::from_nanos(16_666_667));
    }
    // The poll interval is not part of byte-throughput timing. Feed totals and
    // kernel/read/queue counters retain exact wall stages independent of polling.
    drain.join().map_err(|_| "drain panicked")?;
    owner.worker.join().map_err(|_| "owner panicked")??;
    assert_eq!(owner.handle.parsed_total(), expected);
    assert_eq!(owner.handle.pending_bytes(), 0);
    while let Some(reply) = owner.frames.try_receive() {
        publications.push(reply.timing.report());
        owner.frames.recycle(reply.slot);
    }
    let mut result = owner.handle.stage_report();
    result["publications"] = publications.into();
    result["snapshots"] = snapshots.into();
    result["poll_elapsed_ms"] = (started.elapsed().as_secs_f64() * 1000.0).into();
    // Fixed pool ownership remains independent of renderer deadlines or late answers.
    result["owned_pool_bytes"] = (pty_pool::POOL_COUNT * pty_pool::BUFFER_SIZE).into();
    Ok(result)
}

/// Five real-PTY trials per cohort retain read-size and contention evidence.
#[test]
#[ignore = "explicit release measurement over a host-supplied Unicode corpus"]
fn unicode_pty_stage_measurements() -> Result<(), String> {
    let source = std::env::var_os("PUZZLE_UNICODE_CORPUS").ok_or("set PUZZLE_UNICODE_CORPUS")?;
    let corpus = std::fs::read(source).map_err(|e| e.to_string())?;
    if corpus.is_empty() {
        return Err("empty corpus".into());
    }
    let mut bytes = Vec::new();
    while bytes.len() < 8 * 1024 * 1024 {
        bytes.extend_from_slice(&corpus);
    }
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/m16-5");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let path = directory.join("stream.vt");
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    let mut trials = Vec::new();
    for snapshots in [false, true] {
        for _ in 0..5 {
            trials.push(trial(&path, bytes.len() as u64 + 8, snapshots)?);
        }
    }
    std::fs::write(
        directory.join("pty-stages.json"),
        serde_json::to_vec_pretty(&trials).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
