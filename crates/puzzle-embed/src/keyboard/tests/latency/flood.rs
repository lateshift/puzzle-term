//! Real PTY output saturates the unchanged pool while RawInput keys enter its writer.
//! Every key retains wall time and the observer's paired owner CPU interval.
//! Output is the existing truecolor corpus, repeated to the existing 100-MB flood size.
//! No display, fake key transport or alternative parse slice participates.
use super::*;
use std::{path::PathBuf, time::Duration};

/// Creates a finite output file and raw child outside both key and parser timing.
fn source() -> (crate::pty::SpawnedPty, PathBuf, u64) {
    let corpus = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fixtures/corpus/flood.vt"
    ))
    .unwrap();
    let bytes = corpus.repeat(100_000_000usize.div_ceil(corpus.len()));
    let path = std::env::temp_dir().join(format!("puzzle-key-flood-{}.vt", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.args(["-c", "stty raw -echo; printf READY; cat \"$PUZZLE_KEY_FLOOD\" & producer=$!; dd bs=1 count=1053 of=/dev/null 2>/dev/null; wait \"$producer\""]);
    command.env("PUZZLE_KEY_FLOOD", &path);
    let mut child = crate::pty::spawn(
        command,
        PtySize {
            columns: 282,
            rows: 63,
            pixel_width: 2820,
            pixel_height: 1260,
        },
    )
    .unwrap();
    let mut ready = [0; 5];
    child.reader.read_exact(&mut ready).unwrap();
    assert_eq!(&ready, b"READY");
    child.transport.start_writer().unwrap();
    (child, path, bytes.len() as u64)
}

/// Owned finite fixture resources have no shared mutable terminal state.
struct Flood {
    /// Source mutation and public control endpoint remain separate owners.
    owner: session_thread::Started,
    /// Fixed observation atoms are shared with the actual PTY transport.
    metrics: Arc<crate::pipeline_metrics::Metrics>,
    /// RawInput timestamps exercise the original native adapter budget.
    timing: Arc<Mutex<(Instant, Vec<f64>)>>,
    /// The finite reader consumes exactly all encoded keys before normal exit.
    child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Holding EOF keeps the last input control queryable without lifecycle races.
    release: std::sync::mpsc::Sender<()>,
    /// The ordinary producer owns only PTY reads and fixed pool allocations.
    drain: std::thread::JoinHandle<()>,
}

/// Installs observers before transferring the real transport to the sole Session owner.
fn start_owner(mut child: crate::pty::SpawnedPty) -> Flood {
    let (mut producer, consumer) = crate::pty_pool::pair();
    let metrics = consumer.metrics_handle();
    child.transport.set_metrics(metrics.clone());
    let timing = Arc::new(Mutex::new((Instant::now(), Vec::with_capacity(351))));
    let session = Session::new(
        SessionConfig {
            columns: 282,
            rows: 63,
            ..SessionConfig::default()
        },
        Services::new(
            Box::new(TimedTransport {
                inner: child.transport,
                timing: timing.clone(),
            }),
            Box::new(NoDisplay),
        ),
    );
    let owner =
        session_thread::start(session, consumer, session_thread::Config::default()).unwrap();
    // The reader never retains Session; EOF is held until the control observations finish.
    let (release, held) = std::sync::mpsc::channel();
    let drain = std::thread::spawn(move || {
        let mut reader = child.reader;
        while producer.read_once(reader.as_mut()).is_ok_and(|v| v) {}
        held.recv().unwrap();
    });
    Flood {
        owner,
        metrics,
        timing,
        release,
        drain,
        child: child.child,
    }
}

/// Applies the callback-shaped adapter and observes each actual transport handoff.
fn keys(owner: &session_thread::Started, timing: &Arc<Mutex<(Instant, Vec<f64>)>>, expected: u64) {
    let mut adapter = Adapter::default();
    for _ in 0..351 {
        let input = named("ArrowUp");
        let arrival = Instant::now();
        timing.lock().unwrap().0 = arrival;
        let command = adapter.apply(input).unwrap();
        owner
            .handle
            .send_arrived(
                puzzle_term::SessionMessage::Command(command, Origin::User),
                arrival,
            )
            .unwrap();
        // This barrier is outside key timing; the actual LocalPty enqueue ends it.
        owner
            .handle
            .probe(Instant::now() + Duration::from_secs(1), |_| ())
            .unwrap();
        assert!(
            owner.handle.parsed_total() < expected,
            "flood finished before a measured key"
        );
    }
}

/// Every warmed and initial key keeps the unchanged 0.5-ms end-to-end ceiling.
#[cfg_attr(debug_assertions, ignore = "native key budget requires optimized code")]
#[test]
fn key_to_actual_writer_queue_under_flood() {
    // Keep every over-budget pair in the test log, beyond the fixed worst-five tail.
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .try_init();
    let (child, path, expected) = source();
    let Flood {
        owner,
        metrics,
        timing,
        release,
        drain,
        mut child,
    } = start_owner(child);
    let deadline = Instant::now() + Duration::from_secs(1);
    while owner.handle.parsed_total() == 0 {
        assert!(Instant::now() < deadline, "output did not reach the owner");
        std::thread::yield_now();
    }
    keys(&owner, &timing, expected);
    let samples = timing.lock().unwrap().1.clone();
    assert_eq!(samples.len(), 351);
    let mut report = metrics.control.report();
    report["all_raw_input_to_local_pty_wall_ms"] = serde_json::json!(samples);
    report["source_bytes"] = expected.into();
    // Retain evidence before assertions, so every failure has its actual paired clocks.
    save(&report);
    release.send(()).unwrap();
    child.wait().unwrap();
    drain.join().unwrap();
    owner.worker.join().unwrap().unwrap();
    assert_eq!(owner.handle.parsed_total(), expected);
    assert_eq!(owner.handle.pending_bytes(), 0);
    std::fs::remove_file(path).unwrap();
    assert_eq!(report["queued_keys"], 351);
    assert_eq!(report["keys_over_500_us"], 0, "{report}");
    assert!(
        samples.iter().all(|v| *v <= 0.5),
        "raw input queue budget failed: {report}"
    );
}

/// Keeps failed and successful invocations distinct instead of overwriting an outlier.
fn save(report: &serde_json::Value) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/m16-7/keys");
    std::fs::create_dir_all(&root).unwrap();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::fs::write(
        root.join(format!("flood-{}-{stamp}.json", std::process::id())),
        serde_json::to_vec_pretty(report).unwrap(),
    )
    .unwrap();
}
