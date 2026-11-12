//! M15.1 image-free parser floors and chronological throughput observations.
//! Existing release examples retain all five trials before reporting a median.
//! These checks do not substitute legacy clustering for the Unicode Session
//! budget, and they never lower a floor because graphics support was added.
//!
//! The corpus examples measure byte-to-grid work rather than a UTF-8 decoder:
//! ANSI state changes, writes, wrapping and history recycling are all timed.
//! The repeated source is immutable during a trial. Warm-up fills the bounded
//! row storage before five complete chronological feeds are measured. Compiler
//! startup and result-file writes cannot contribute to the observed parse rate.
//!
//! The Unicode probe additionally compares VT with Session reconciliation and
//! keeps the configured grapheme policy explicit. Legacy mode is useful evidence
//! but cannot satisfy the clustering requirement. Its three slice sizes describe
//! kernel-read granularity, the desktop owner slice and the core budgeted feed;
//! combining them would hide a fixed per-feed cost on macOS's small PTY reads.
//!
//! These are task-level checks of image-free regressions. They make no claim
//! about image upload cost, presented fps or input under a graphics flood; later
//! M15 tasks own those separate end-to-end criteria. In particular a headless
//! pass cannot certify either Chrome pixels or a native app's live image view.
//!
//! Diagnostic rows deliberately have no invented throughput floor. Their raw
//! samples are retained for the before/after report rather than turning a noisy
//! short measurement into a new unapproved performance rule (R5/R14).
use crate::accept::report;
use std::{fs, path::PathBuf, process::Command};

/// Each corpus keeps its own result; a fast average cannot conceal a slow one.
pub(super) fn run(passed: &mut bool) {
    // A timestamped destination preserves every attempt, including failed floors.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_nanos());
    let out = PathBuf::from(format!("target/m15-4/{stamp}-parse"));
    println!("M15_EVIDENCE {}", out.display());
    for (name, minimum) in [
        ("plain-ascii", 250.0),
        ("sgr-heavy", 100.0),
        ("cursor-heavy", 80.0),
        ("unicode-mix", 0.0),
        ("flood", 0.0),
        ("tmux-single", 80.0),
        ("tmux-hsplit", 80.0),
        ("tmux-vsplit", 80.0),
    ] {
        // Unicode mix and flood parse rate are diagnostic, not the distinct
        // Unicode Session and presented-fps criteria defined by §15.
        report(
            &format!("M15.1 image-free {name} parse"),
            corpus(name, minimum, &out),
            passed,
        );
    }
    report(
        "M15.1 Unicode Session clustering >=200 MiB/s",
        unicode(&out),
        passed,
    );
}

/// Source generation, compilation and file I/O are outside every sample clock.
fn corpus(name: &str, minimum: f64, out: &std::path::Path) -> Result<(), String> {
    // The original examples remove stale median artifacts before each run.
    let rate = if name == "plain-ascii" {
        crate::corpus::ascii_rate()?
    } else {
        crate::corpus::mixed_rate(name)?
    };
    // Print exact units; MB/s corpora and MiB/s Unicode are intentionally distinct.
    println!("M15_METRIC {name}={rate:.3} MB/s minimum={minimum:.0}");
    // Copy chronological receipts before another task can overwrite target/corpus.
    fs::create_dir_all(out).map_err(|e| e.to_string())?;
    // The median and chronological file are distinct observations: retain
    // both, rather than reconstructing five identical samples from one number.
    for suffix in ["mbps.txt", "samples.txt"] {
        let file = format!("{name}-{suffix}");
        // Plain ASCII's original probe may report just its median, unlike the
        // mixed example. Missing optional chronological data is not fabricated.
        let path = PathBuf::from("target/corpus").join(&file);
        if path.is_file() {
            fs::copy(path, out.join(file)).map_err(|e| e.to_string())?;
        }
    }
    // A nonpositive diagnostic is invalid evidence even without a fixed floor.
    if !rate.is_finite() || rate <= 0.0 || rate < minimum {
        return Err(format!("{rate:.3} MB/s below {minimum:.0}"));
    }
    Ok(())
}

/// Match M16's real corpus and headless geometry, including all slice sizes.
fn unicode(out: &std::path::Path) -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    // Host-provided input stays external; no downloaded or synthetic replacement
    // may stand in for the Unicode corpus used by the approved budget.
    let source = std::env::var_os("PUZZLE_UNICODE_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            root.parent()
                .unwrap_or(&root)
                .join("vtebench/benchmarks/unicode/symbols")
        });
    if !source.is_file() {
        return Err("installed vtebench Unicode corpus absent".into());
    }
    crate::commands::quiet(
        "cargo",
        &[
            "build",
            "--release",
            "-p",
            "puzzle-term",
            "--example",
            "throughput_bench",
        ],
    )?;
    fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let output = out.join("unicode-stages.csv");
    // Remove the old file, so an executable that fails to write cannot pass.
    if output.exists() {
        fs::remove_file(&output).map_err(|e| e.to_string())?;
    }
    let status = Command::new("target/release/examples/throughput_bench")
        .arg(source)
        .arg(&output)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        // A partial result from a failed probe is not assessed as a shorter run.
        return Err("headless Unicode probe failed".into());
    }
    assess(&fs::read_to_string(&output).map_err(|e| e.to_string())?)
}

/// A complete five-trial cohort, not an available fast subset, determines each floor.
fn assess(csv: &str) -> Result<(), String> {
    // The example writes one header and sixty rows: five trials times two
    // owners, two clustering policies and three independently measured slices.
    if csv.lines().count() != 61 {
        return Err("headless CSV lacks sixty chronological observations".into());
    }
    for slice in [1024, 16384, 65536] {
        let mut rates = Vec::new();
        for line in csv.lines().skip(1) {
            let fields: Vec<_> = line.split(',').collect();
            // Reject broken serialization instead of shifting columns and
            // accidentally reading elapsed milliseconds as MiB per second.
            if fields.len() != 7 {
                return Err("malformed headless observation".into());
            }
            // The VT-only and legacy-mode observations remain evidence, but
            // only clustering through Session satisfies the approved row.
            if fields[0] == "session" && fields[1] == "true" && fields[2] == slice.to_string() {
                let rate = fields[6].parse::<f64>().map_err(|e| e.to_string())?;
                // NaN would make ordering/threshold comparisons meaningless.
                if !rate.is_finite() || rate <= 0.0 {
                    return Err("invalid Unicode throughput".into());
                }
                rates.push(rate);
            }
        }
        if rates.len() != 5 {
            // Missing slow samples may not disappear from the median's cohort.
            return Err(format!("{slice}-byte Unicode cohort incomplete"));
        }
        rates.sort_by(f64::total_cmp);
        // All five observations are retained in the original CSV; sorting this
        // temporary vector only selects their middle value, never a best run.
        println!(
            "M15_METRIC Unicode Session slice={slice}: {:.6} MiB/s",
            rates[2]
        );
        if rates[2] < 200.0 {
            return Err(format!("{slice}-byte median {} <200 MiB/s", rates[2]));
        }
    }
    Ok(())
}
