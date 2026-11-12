//! M7.7 measures optimized host seams, real browser parsing and shipped size.
//!
//! Every browser metric is backed by an exact executed witness in the one
//! shared Chrome run. Native timing uses release code and an actual PTY queue.
//! Development fixtures must never enter the normal distribution's bindings.
use super::super::report;
use std::{fs, path::Path};

/// Browser criteria share the production suite rather than launching per metric.
pub(super) fn browser(output: &Result<String, String>, passed: &mut bool) {
    // Input measures mapper, shared policy and byte encoding, not display latency.
    // The two parser floors include writes and scrolling, not a decode-only sink.
    // Frame construction is warmed separately from parser and GPU submission cost.
    // The transfer witness checks bytes as well as cadence and queued input.
    for (criterion, name) in [
        (
            "Chrome key-to-transport maximum <= 0.5 ms",
            "runtime::tests::measurements::dom_key_to_transport_budget",
        ),
        (
            "Chrome ASCII parse/grid >= 100 MB/s",
            "benchmarks::tests::ascii_parse_budget",
        ),
        (
            "Chrome SGR parse/grid >= 40 MB/s",
            "benchmarks::tests::sgr_parse_budget",
        ),
        (
            "Chrome full/one-row frame CPU <= 4/0.5 ms",
            "benchmarks::tests::frame_cpu_budgets",
        ),
        (
            "100 MB browser checksum, >=30 submitted fps, FIFO key and <=4 MiB pending",
            "runtime::tests::measurements::bridge_100mb_keeps_presenting_and_input_flows",
        ),
    ] {
        // Cargo's empty-filter success cannot substitute for an executed witness.
        // Reuse the complete suite status: a failure cannot be hidden by one PASS string.
        // Each name is qualified to distinguish similar native or headless tests.
        let result = output.as_ref().map_err(Clone::clone).and_then(|text| {
            if text.contains(&format!("test {name} ... ok")) {
                // The test itself owns its unchanged specified threshold.
                Ok(())
            } else {
                // Report a removed or renamed witness as a missing measurement.
                Err(format!("browser measurement {name} did not run"))
            }
        });
        report(criterion, result, passed);
    }
    // Real bridge diagnostics prove the producer was paused, not just throttled by cat.
    // This file is recreated at startup of this same suite, avoiding stale evidence.
    // Debug events name the credit decision without containing PTY bytes or tokens.
    let pauses = fs::read_to_string("target/m7-browser-bridge.log")
        .map_err(|e| e.to_string())
        .and_then(|log| {
            // Seeing output packets alone would not prove real producer backpressure.
            // A paused coordinator event is emitted when outstanding credit is full.
            log.contains("bridge.backpressure")
                .then_some(())
                .ok_or_else(|| "no output-credit pause in the real bridge log".into())
        });
    report(
        "browser flood produces bridge backpressure pauses",
        pauses,
        passed,
    );
    // Failed runs retain their complete sanitized log through the suite driver.
    // Successful runs expose concise values for README.md's budget table.
    if let Ok(text) = output {
        // Retain measurements in the gate output without echoing credentials or compiler chatter.
        for line in text.lines().filter(|line| line.contains("M7_METRIC")) {
            println!("{line}");
        }
    }
}

/// Optimized native mapping/policy/encoding ends at the actual ordered writer queue.
fn desktop() -> Result<(), String> {
    // Release avoids the instruction overhead of unoptimized domain code.
    // Release-only owner/transport observations use the sanctioned metrics feature.
    let name = "keyboard::tests::latency::key_to_queued_pty_budget";
    // The test must overwrite its evidence this run; stale files are never accepted.
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
        ],
    )?;
    // Successful process exit is insufficient when Cargo has matched zero tests.
    // A fresh exact execution also guarantees the metrics file was rewritten.
    if !output.contains(&format!("test {name} ... ok")) {
        return Err("desktop latency witness did not run".into());
    }
    // Printing happens in xtask, after timing, never in the library input lane.
    // The artifact reports median, p99 and the full observed warmed maximum.
    // The assertion uses the maximum; no outlier is trimmed from acceptance.
    let metrics = fs::read_to_string("target/m7-desktop-input.txt").map_err(|e| e.to_string())?;
    println!("M7_METRIC desktop {metrics}");
    Ok(())
}

/// Measures Trunk's normal optimized artifact without enabling any dev feature.
fn shipped() -> Result<(), String> {
    // Never measure the benchmark page: it intentionally embeds test-only fonts/corpora.
    // The default demo pulls in the complete engine with the production feature set.
    crate::commands::web(Some("build"))?;
    // Trunk content hashes filenames, so discover the emitted artifact by extension.
    let paths = fs::read_dir("dist/web").map_err(|e| e.to_string())?;
    // Exactly the discovered production wasm supplies the compressed size below.
    let mut wasm = None;
    for entry in paths {
        let path = entry.map_err(|e| e.to_string())?.path();
        // The generated JS is also a direct guard against accidentally shipping probes.
        if path.extension().is_some_and(|ext| ext == "js") {
            let js = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            if [
                // Corpus/frame entry points embed fixture bytes and belong only to dev builds.
                "benchmarkParse",
                "benchmarkFrame",
                "benchmarkScroll",
                // The incremental search owner also pulls in the neutral 100k-line fixture.
                "SearchBenchmark",
                // Start/stop recording methods must not extend the shipped terminal API.
                "measureScrollStart",
                "measureScrollFrames",
            ]
            .iter()
            .any(|name| js.contains(name))
            {
                return Err("development benchmark binding in shipped JS".into());
            }
        }
        // Separate font resources are intentionally not counted as wasm payload.
        if path.extension().is_some_and(|ext| ext == "wasm") {
            wasm = Some(path);
        }
    }
    // Compression includes stream headers and uses the already installed brotli tool.
    size(&wasm.ok_or("missing shipped wasm")?)
}

/// The target is reported; only the specified hard limit makes acceptance fail.
fn size(path: &Path) -> Result<(), String> {
    // Command arguments are passed directly; filesystem punctuation is not shell code.
    let path = path.to_str().ok_or("non-UTF-8 wasm path")?;
    let bytes = crate::commands::compressed("brotli", &["-c", path])?;
    // R5 forbids changing optimization or starting size work merely to hit the target.
    println!("M7_METRIC shipped_brotli_bytes={bytes} target=1000000 hard_limit=1500000");
    // Decimal MB matches the spec table's 1,000,000 and 1,500,000 byte bounds.
    // Target overshoot is informational; a hard-limit breach requires the user's decision.
    if bytes > 1_500_000 {
        Err(format!(
            "shipped wasm {bytes} B exceeds the 1.5 MB hard limit"
        ))
    } else {
        Ok(())
    }
}

/// Measurements are part of the mechanical gate; human interaction remains R15 review.
pub(super) fn run(passed: &mut bool) {
    // Keep native latency separate so a browser driver failure cannot conceal its result.
    report(
        "desktop key-to-transport maximum <= 0.5 ms",
        desktop(),
        passed,
    );
    // Only the normal feature set is measured for the shipped product's size.
    report(
        "full shipped wasm <=1.5 MB brotli and excludes dev bindings",
        shipped(),
        passed,
    );
}
