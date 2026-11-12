//! Surviving D-29 copy budget and D-50 flood publication observations.
//! Request timing includes owner queue residence; it is never labelled a lock.
use super::*;
use puzzle_term::{
    FrameResult, PublishOutcome, frame_builder::context::TextContext, frame_snapshot::FrameSnapshot,
};

/// Constructs measured font geometry before moving Session to its owner.
fn font_session(config: SessionConfig) -> Result<(Session, TextContext), String> {
    let (writes, _) = mpsc::channel();
    let mut session = Session::new(
        config,
        Services::new(Box::new(Writer(writes)), Box::new(Quiet)),
    );
    let context = TextContext::new(crate::font_data::bundled_font_set()?, 14.0, 1.0)?;
    session.set_text_context(&context);
    Ok((session, context))
}

/// Populates every cell before either the copy-cost or allocation witness runs.
fn full_screen() -> Result<(Session, TextContext), String> {
    let (mut session, context) = font_session(SessionConfig {
        columns: 300,
        rows: 80,
        ..SessionConfig::default()
    })?;
    let mut screen = Vec::with_capacity(80 * 302);
    for row in 0..80 {
        screen.extend(std::iter::repeat_n(b'A' + row % 26, 300));
        if row != 79 {
            screen.extend_from_slice(b"\r\n");
        }
    }
    let mut outcome = session.feed(&screen, Instant::now());
    while outcome.pending_bytes > 0 {
        outcome = session.feed(&[], Instant::now());
    }
    Ok((session, context))
}

/// Debug and optimized checks retain full recapture and allocation assertions (P1).
#[test]
fn full_snapshot_recapture_is_complete_and_allocation_free() -> Result<(), String> {
    let (mut session, context) = full_screen()?;
    let mut snapshot = FrameSnapshot::default();
    // Warm the existing 64-entry VT journal and consume host events as a host does.
    for _ in 0..64 {
        recapture(&mut session, &context, &mut snapshot)?;
        while session.take_event().is_some() {}
    }
    let allocations = allocation_counter::measure(|| {
        for _ in 0..101 {
            assert!(recapture(&mut session, &context, &mut snapshot).is_ok());
            assert_eq!(snapshot.copied_count(), 80);
            while session.take_event().is_some() {}
        }
    });
    assert_eq!(allocations.count_total, 0);
    Ok(())
}

/// Forces recovery through the public owner API, keeping every row damaged.
fn recapture(
    session: &mut Session,
    context: &TextContext,
    snapshot: &mut FrameSnapshot,
) -> Result<(), String> {
    session.apply(
        SessionMessage::FrameResult(FrameResult::BuildFailed),
        Instant::now(),
    )?;
    assert_eq!(
        session.publish_frame(Instant::now(), snapshot, context.atlas_generation(), 0)?,
        PublishOutcome::Published
    );
    assert_eq!(snapshot.copied_count(), 80);
    Ok(())
}

/// Measures the unchanged 500-µs p99 budget in optimized code (§15.2, reviewer R5).
#[cfg_attr(
    debug_assertions,
    ignore = "M16.7 snapshot timing budget requires the release profile"
)]
#[test]
fn full_snapshot_copy_p99() -> Result<(), String> {
    let (mut session, context) = full_screen()?;
    let mut snapshot = FrameSnapshot::default();
    let mut samples = Vec::with_capacity(101);
    for _ in 0..101 {
        // Failed-build recovery recaptures every row rather than timing an idle answer.
        session.apply(
            SessionMessage::FrameResult(FrameResult::BuildFailed),
            Instant::now(),
        )?;
        let cpu = crate::thread_cpu::now();
        let began = Instant::now();
        assert_eq!(
            session.publish_frame(began, &mut snapshot, context.atlas_generation(), 0)?,
            PublishOutcome::Published
        );
        let wall = began.elapsed();
        samples.push((wall, crate::thread_cpu::elapsed(cpu)));
        assert_eq!(snapshot.copied_count(), 80);
    }
    snapshot_report(&samples)?;
    samples.sort_unstable_by_key(|sample| sample.0);
    let p99 = samples[(samples.len() - 1) * 99 / 100].0.as_micros();
    crate::test_support::report(format_args!("snapshot_copy_p99_us={p99}"));
    assert!(p99 <= 500, "full snapshot copy exceeded 0.5 ms: {p99} µs");
    Ok(())
}

/// Keeps every copy attempt, including debug/check outliers, beside its same-thread CPU span.
fn snapshot_report(samples: &[(Duration, Option<Duration>)]) -> Result<(), String> {
    let summary = serde_json::json!({
        "wall_ms": copy_distribution(samples.iter().map(|sample| sample.0)),
        "thread_cpu_ms": copy_distribution(samples.iter().filter_map(|sample| sample.1)),
    });
    crate::test_support::report(format_args!("snapshot_copy_distribution={summary}"));
    let samples = samples
        .iter()
        .map(|(wall, cpu)| {
            serde_json::json!({
                "wall_ms": wall.as_secs_f64()*1000.0,
                "thread_cpu_ms": cpu.map(|v| v.as_secs_f64()*1000.0),
            })
        })
        .collect::<Vec<_>>();
    // Formatting and filesystem work follow all timed copies and cannot enter a sample.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/m16-7/copies");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    std::fs::write(
        root.join(format!("{}-{stamp}.json", std::process::id())),
        serde_json::to_vec_pretty(&serde_json::json!({
            "optimized": !cfg!(debug_assertions), "summary": summary, "samples": samples
        }))
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Percentiles use all observations, including initial allocation and cold copies.
fn copy_distribution(samples: impl Iterator<Item = Duration>) -> serde_json::Value {
    let mut samples = samples
        .map(|v| v.as_secs_f64() * 1000.0)
        .collect::<Vec<_>>();
    samples.sort_unstable_by(f64::total_cmp);
    if samples.is_empty() {
        return serde_json::Value::Null;
    }
    let last = samples.len() - 1;
    serde_json::json!({"p50": samples[last / 2], "p99": samples[last * 99 / 100], "max": samples[last]})
}

/// A truecolor flood records every request answer with paired owner clocks.
#[test]
fn flood_request_publication_distribution() -> Result<(), String> {
    let corpus = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fixtures/corpus/flood.vt"
    ))
    .map_err(|error| error.to_string())?;
    let bytes = corpus.repeat((8_000_000 / corpus.len()).max(1));
    let expected = bytes.len() as u64;
    let (session, context) = font_session(SessionConfig::default())?;
    let (mut producer, consumer) = crate::pty_pool::pair();
    let started = start(session, consumer, Config::default())?;
    let (release, hold_eof) = mpsc::channel();
    let source = thread::spawn(move || -> Result<(), String> {
        let mut reader = Cursor::new(bytes);
        while producer.read_once(&mut reader)? {}
        hold_eof.recv().map_err(|error| error.to_string())?;
        Ok(())
    });
    let samples = observe_flood(&started, context.atlas_generation(), expected)?;
    release.send(()).map_err(|error| error.to_string())?;
    source.join().map_err(|_| "flood producer panicked")??;
    started
        .worker
        .join()
        .map_err(|_| "flood owner panicked")??;
    assert_eq!(started.handle.parsed_total(), expected);
    assert_eq!(started.handle.pending_bytes(), 0);
    assert!(
        !samples.is_empty(),
        "flood supplied no publication observations"
    );
    // Receipts must also work in a fresh checkout, before any M16 probe ran.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/m16-5");
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let path = root.join("flood-publications.json");
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&samples).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    retain_publications(&samples)?;
    crate::test_support::report(format_args!("publication_samples={}", samples.len()));
    Ok(())
}

/// The acceptance-compatible fixed path is supplemented by an append-only run receipt.
fn retain_publications(samples: &[serde_json::Value]) -> Result<(), String> {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/m16-7/publications");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    std::fs::write(
        root.join(format!("{}-{stamp}.json", std::process::id())),
        serde_json::to_vec_pretty(samples).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Polling is a finite measurement refresh source, with the same one-period wait bound.
fn observe_flood(
    started: &Started,
    generation: u64,
    expected: u64,
) -> Result<Vec<serde_json::Value>, String> {
    let end = Instant::now() + crate::test_support::HANG_GUARD;
    let period = Duration::from_nanos(16_666_667);
    let mut samples = Vec::new();
    while started.handle.parsed_total() < expected && Instant::now() < end {
        let now = Instant::now();
        started
            .handle
            .request_frame(now, generation, 0, now + period)?;
        if let Some(answer) = started.frames.receive_until(now + period) {
            samples.push(answer.timing.report());
            started.frames.recycle(answer.slot);
        }
    }
    // Preserve any completed late answers instead of selecting only punctual ones.
    while let Some(answer) = started.frames.try_receive() {
        samples.push(answer.timing.report());
        started.frames.recycle(answer.slot);
    }
    assert_eq!(started.handle.parsed_total(), expected, "flood timed out");
    Ok(samples)
}
