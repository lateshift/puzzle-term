//! Four approved §15 rows retain independent measurements and failure reports.
//! Headless throughput includes clustering through Session; tmux measures the
//! actual captured redraw streams. The maximized PTY result uses vtebench's
//! producer timestamps, while warm frame cost comes from native build clocks.
//! Decimal MB/s and binary MiB/s are intentionally kept distinct.
//! A human compares the reference terminal at matched geometry; native counters cannot certify
//! its appearance or silently replace that required review with a byte rate.
use super::super::super::report;
use std::{fs, process::Command};

/// Frame ceilings and observed submission cost share the same warm interval.
pub(in crate::accept) mod frames;

/// Independent failures never prevent later approved budgets from being checked.
pub(super) fn run(passed: &mut bool) {
    // The Session median is the approved headless row, including mode 2027
    // and grid writes. A pure UTF-8 decoder rate would not meet that contract.
    report(
        "M16.5 headless Unicode clustering ≥200 MiB/s",
        headless(),
        passed,
    );
    // Capacity crossing still exercises the production pool and owner, while
    // D-50 explicitly withdraws the shared-lock timing criterion.
    report(
        "M16.5 sparse history capacity preserves byte cap and zero deque relocation (D-50)",
        capacity(),
        passed,
    );
    // These three real tmux pane layouts each own an independent hard floor.
    // An average across them could conceal the slower single or vertical stream.
    for name in ["tmux-single", "tmux-hsplit", "tmux-vsplit"] {
        report(&format!("M16.5 {name} ≥80 MB/s"), tmux(name), passed);
    }
    // PTY backpressure is measured by the real producer, separately from
    // headless parsing and queue residence observations in the stage report.
    report("M16.5 maximized Unicode PTY ≥130 MiB/s", live(), passed);
    // The 120-Hz allowance is derived from observed upload/submit cost,
    // even when this matched-geometry window is on the external 60-Hz monitor.
    report(
        "M16.5 warm new-Unicode frame median/120-Hz allowance",
        frames::run(),
        passed,
    );
}

/// Checks every Session slice size, retaining all five chronological observations.
/// VT-only and legacy samples remain in the evidence but cannot satisfy this row.
/// A slower first sample is retained in the median rather than deleted as warmup.
fn headless() -> Result<(), String> {
    // stages() generates this complete table earlier in the same invocation.
    // It retains both owners and policies, allowing a reviewer to compare
    // the clustering cost with legacy without letting legacy satisfy its floor.
    let csv = fs::read_to_string("target/m16-5/accept-headless.csv").map_err(|e| e.to_string())?;
    for size in [1024, 16384, 65536] {
        // Each offered slice size is a separate five-trial cohort; combining
        // sizes would hide a slow small-read path used by macOS PTYs.
        let mut values = Vec::new();
        for line in csv.lines().skip(1) {
            let fields: Vec<_> = line.split(',').collect();
            // Typed fields reject truncated output and keep policy/owner explicit.
            if fields.len() != 7 {
                return Err("headless CSV malformed".into());
            }
            if fields[0] == "session"
                && fields[1] == "true"
                && fields[2].parse::<usize>().map_err(|e| e.to_string())? == size
            {
                values.push(fields[6].parse::<f64>().map_err(|e| e.to_string())?);
            }
        }
        // The fixed trial count rejects a truncated run or accidentally
        // duplicated samples rather than trusting an available fast subset.
        if values.len() != 5 {
            return Err(format!("{size}-byte Session cohort incomplete"));
        }
        // Percentiles reject NaN, infinity and negative rates before sorting.
        // Binary units match the user's Unicode row; tmux below uses decimal MB.
        let median = frames::percentile(&mut values, 0.5)?;
        println!("M16.5 headless Unicode Session slice={size}: {median:.3} MiB/s");
        if median < 200.0 {
            return Err(format!("{size}-byte Session median {median:.3} <200 MiB/s"));
        }
    }
    Ok(())
}

/// The real parser/pool regression crosses the sparse 32-MiB history ceiling.
/// Byte equality, retained history and zero deque-relocation assertions stay intact.
/// The exact named test must run; an empty Cargo filter is not evidence.
fn capacity() -> Result<(), String> {
    // Explicit release mode keeps debug allocation and assertions from defining
    // the native timing budget. lock-metrics activates the production observer;
    // --ignored selects the finite capacity benchmark excluded from unit loops.
    let output = Command::new("cargo")
        .args([
            "test",
            "--release",
            "-p",
            "puzzle-embed",
            "--features",
            "lock-metrics",
            "session_thread::tests::growth::sparse_history_capacity_preserves_bounds",
            "--",
            "--exact",
            "--ignored",
        ])
        .output()
        .map_err(|e| e.to_string())?;
    // Preserve both streams, including a failed report written before its assertion.
    let mut bytes = output.stdout;
    bytes.extend_from_slice(&output.stderr);
    fs::write("target/m16-5/accept-capacity.log", &bytes).map_err(|e| e.to_string())?;
    // A command can exit zero after selecting no test. Require the named test
    // completion as well as the process status before certifying this crossing.
    if !output.status.success()
        || !String::from_utf8_lossy(&bytes).contains(
            "test session_thread::tests::growth::sparse_history_capacity_preserves_bounds ... ok",
        )
    {
        return Err("capacity storage regression failed; target/m16-5/accept-capacity.log".into());
    }
    Ok(())
}

/// Rebuild the standard probe before timing; remove each old numerical artifact.
/// The existing five-sample parser median uses actual captured pane geometry.
/// Each corpus reports independently so one failure cannot hide a second defect.
fn tmux(name: &str) -> Result<(), String> {
    // Reusing the existing parse probe keeps its actual 160×48 tmux geometry,
    // full recorded control bytes and five-sample median. Compilation ends
    // before its internal clock starts; no render or PTY participates.
    crate::commands::quiet(
        "cargo",
        &[
            "build",
            "--release",
            "-p",
            "puzzle-vt",
            "--example",
            "parse_budget",
        ],
    )?;
    // The probe writes only after timing. Delete an old number first so a
    // failed run cannot leave a passing measurement from an earlier binary.
    let path = format!("target/corpus/{name}-mbps.txt");
    let _ = fs::remove_file(&path);
    crate::commands::quiet("target/release/examples/parse_budget", &[name])?;
    // Interpret the probe's decimal MB/s artifact without converting it to
    // Unicode's MiB/s. The user approved separate units for these two rows.
    let value = fs::read_to_string(&path)
        .map_err(|e| e.to_string())?
        .trim()
        .parse::<f64>()
        .map_err(|e| e.to_string())?;
    println!("M16.5 {name}: {value:.3} MB/s");
    if !value.is_finite() || value < 80.0 {
        return Err(format!("{value:.3} <80 MB/s"));
    }
    Ok(())
}

/// vtebench repeats complete source files until its one-MiB minimum is reached.
/// Reconstruct that byte count from the same installed corpus, never the rounded
/// UI label “1.06 MiB”. All producer samples enter the average, including cold ones.
pub(in crate::accept) fn live() -> Result<(), String> {
    // The matched report must contain GPU ink samples; a closed window or
    // timing-only cohort cannot supply the producer table for this criterion.
    let value = frames::matched()?;
    let dat = fs::read_to_string("target/m16-2/bitmap-maximized-vtebench.dat")
        .map_err(|e| e.to_string())?;
    let mut lines = dat.lines().map(str::trim);
    if lines.next() != Some("unicode") {
        return Err("Unicode producer table absent".into());
    }
    // vtebench records integer milliseconds with whitespace in its dat table.
    // Preserve every cold and warm observation and reject invalid durations.
    let mut samples = Vec::new();
    for line in lines {
        samples.push(line.parse::<f64>().map_err(|e| e.to_string())?);
    }
    if samples.is_empty() || samples.iter().any(|n| !n.is_finite() || *n <= 0.0) {
        return Err("invalid vtebench producer samples".into());
    }
    let size = fs::metadata(super::corpus()?)
        .map_err(|e| e.to_string())?
        .len();
    if size == 0 {
        return Err("empty Unicode corpus".into());
    }
    // The actual symbols file is repeated whole, including its final bytes,
    // until at least one MiB is offered. The rounded display label overstates
    // neither the numerator nor the duration used for this hard floor.
    let bytes = size * (1024 * 1024_u64).div_ceil(size);
    // Average duration matches vtebench's displayed result; averaging rates
    // instead would overweight fast samples and change the comparison.
    let mean_ms = samples.iter().sum::<f64>() / samples.len() as f64;
    let rate = bytes as f64 / (1024.0 * 1024.0) / (mean_ms / 1000.0);
    println!(
        "M16.5 maximized Unicode: {mean_ms:.6} ms/{bytes} B = {rate:.3} MiB/s; {} samples; reference user baseline 6.02 ms, fresh matched user comparison pending",
        samples.len()
    );
    // The complete producing report ties the table to an actual observed native window.
    if value["seconds"]
        .as_f64()
        .is_none_or(|seconds| seconds < 30.0)
        || rate < 130.0
    {
        return Err(format!("incomplete live cohort or {rate:.3} <130 MiB/s"));
    }
    Ok(())
}
