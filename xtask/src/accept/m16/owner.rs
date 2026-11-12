//! D-50's shared-lock replacement and surviving key-to-writer budget.
//!
//! Structural ownership is the criterion: missing locks are not zero-valued
//! latency samples. Publication observations reuse the same validator as M6,
//! which requires fresh chronological wall/CPU pairs and deadline flags.
//! The key witness starts at the native RawInput adapter and ends at LocalPty's
//! actual ordered-writer queue, under a real 100-MB PTY flood. A memory transport
//! could hide the queue copy or send, so it cannot satisfy this acceptance line.
//! Exact test-name matching rejects Cargo's successful zero-test result.
//! The test saves every initial and warmed key before asserting the unchanged
//! half-millisecond maximum; over-budget pairs remain in its tracing log.
//! CPU timing belongs to the owner application interval, never to a subtraction
//! of unrelated sender and receiver CPU clocks. Control residence is wall-only.
//! The fixed drain pool and production 16-KiB slice are unchanged in the witness.
//!
//! Snapshot timing also executes only in the release profile, as §15.2 defines
//! native performance. Parallel debug checks still force complete recaptures
//! and count allocations in a separate test. Their scheduling/preemption time
//! cannot serve as the optimized copy-cost measurement (reviewer R5 decision).
//! The optimized witness saves every attempt before asserting its p99 ceiling.
//! Its summary prints p50, p99 and maximum for wall and same-thread CPU time.
//! Initial cold copies remain in the distribution; no outliers are discarded.
//! A release failure propagates through capture and remains an acceptance FAIL.
//! Cargo's ignore annotation changes only debug timing, never the 500-µs bound.
//! Requiring the summary marker prevents a test without evidence passing silently.
use super::super::report;

/// Adds both replacement rows and retains the independent end-to-end input ceiling.
pub(super) fn run(passed: &mut bool) {
    report(
        "M16.7 main/render never wait for a Session by construction (D-50)",
        crate::lint::session::check(),
        passed,
    );
    report(
        "M16.7 frame request publications record wall/CPU distribution and late count",
        super::super::m6::threading::parse::publication_distribution(),
        passed,
    );
    report(
        "M16.7 full snapshot copy p99 <=0.5 ms release; paired wall/CPU distribution",
        full_snapshot_copy(),
        passed,
    );
    report(
        "M16.7 RawInput to actual PTY writer queue <=0.5 ms under flood",
        flooded_keys(),
        passed,
    );
}

/// Runs the real owner and transport, including their feature-gated paired-clock observer.
fn flooded_keys() -> Result<(), String> {
    let name = "keyboard::tests::latency::flood::key_to_actual_writer_queue_under_flood";
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "--release",
            "-p",
            "puzzle-embed",
            "--features",
            "lock-metrics",
            name,
            "--",
            "--exact",
            "--nocapture",
        ],
    )?;
    // A deleted witness cannot pass from an empty filter or an ignored debug test.
    if !output.contains(&format!("test {name} ... ok")) {
        return Err("actual-writer flood key witness did not execute".into());
    }
    Ok(())
}

/// Executes the optimized copy gate explicitly; debug checks keep the functional witness.
fn full_snapshot_copy() -> Result<(), String> {
    let name = "session_thread::tests::capture::full_snapshot_copy_p99";
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "--release",
            "-p",
            "puzzle-embed",
            "--features",
            "lock-metrics",
            name,
            "--",
            "--exact",
            "--nocapture",
        ],
    )?;
    // Require both the real test execution and its complete paired-clock summary.
    if !output.contains(&format!("test {name} ... ok"))
        || !output.contains("snapshot_copy_distribution=")
    {
        return Err("release snapshot copy witness did not execute or report clocks".into());
    }
    println!("{output}");
    Ok(())
}
