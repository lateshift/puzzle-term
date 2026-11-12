//! M8.8 retains complete workloads and real clock observations (performance rule P13).
//!
//! Search has a row allowance, not an approved millisecond floor or ceiling.
//! Its native probe times Session turns; Chrome yields at actual refreshes.
//! The final blank row is part of the workload, making 51 slices and 100,001
//! examined rows. Exactly 1,000 authored hits reject a shortened history.
//! Every CPU sample is kept, including the cold first and short final turn.
//! Recorded GPU submissions are checked separately from frame assembly CPU.
//! The hardware recordings can fail on a 60-Hz display even with green tests.
//! That failure is intentional: synthetic timing cannot prove 120-Hz cadence.
//! The browser suite is shared with M8.7 and must execute at both DPR settings.
//! Full logs expose real search CPU and refresh-paced wall time independently.
//! Only xtask formats summaries; no per-slice production logging is introduced.
//! A successful process exit alone is insufficient: a broken probe could emit
//! no rows. Strict parsing rejects missing pairs, non-finite clocks and zero
//! progress, while the independently authored workload fixes the expected sum.
//! Total CPU is the sum of measured turns, excluding fixture setup and waits.
//! Native turns run consecutively; browser turns include one refresh wait each.
//! The two totals therefore describe different clocks and are labelled explicitly.
//! The browser assertion owns its dimensions; acceptance requires its exact name.
//! Separate cadence labels preserve both platform failures in the final inventory.
use super::report;
/// Raw submission recording verification is independent of search timings.
pub(in crate::accept) mod cadence;

/// Measures native slices and validates the independent fixture dimensions.
fn native_search() -> Result<(), String> {
    let output = crate::commands::capture(
        "cargo",
        &[
            "run",
            "--release",
            "-p",
            "puzzle-term",
            "--features",
            "test-scenes",
            "--example",
            "search_bench",
        ],
    )?;
    // Retain every observed pair, not just a rounded median in the gate log.
    std::fs::write("target/m8-search-native.txt", &output).map_err(|e| e.to_string())?;
    let mut samples = Vec::new();
    let mut rows = 0;
    for line in output.lines().filter(|line| line.starts_with("slice_ms=")) {
        let (cpu, count) = line.split_once(" rows=").ok_or("malformed search slice")?;
        let cpu: f64 = cpu[9..].parse().map_err(|_| "invalid search time")?;
        let count: usize = count.parse().map_err(|_| "invalid search row count")?;
        if !cpu.is_finite() || cpu < 0.0 || !(1..=2_000).contains(&count) {
            return Err(format!("invalid search slice: {line}"));
        }
        rows += count;
        samples.push(cpu);
    }
    if samples.len() != 51 || rows != 100_001 || !output.lines().any(|s| s == "count=1000") {
        return Err("search did not finish the full 100,000-line fixture in 51 slices".into());
    }
    let total: f64 = samples.iter().sum();
    // Sorting changes only the summary order, not the retained raw observations.
    samples.sort_by(f64::total_cmp);
    println!(
        "M8_METRIC native search_slice_median_ms={:.6} search_slice_max_ms={:.6} search_cpu_total_ms={total:.6} slices=51 rows=100001 count=1000",
        samples[25], samples[50]
    );
    Ok(())
}

/// Requires the timed browser witness twice, never a successful empty filter.
fn browser_search(output: &Result<String, String>) -> Result<(), String> {
    let text = output.as_ref().map_err(Clone::clone)?;
    let witness = "test benchmarks::tests::search_slice_and_total_measurement ... ok";
    if text.matches(witness).count() != 2 {
        return Err("missing search timing witness at both DPRs".into());
    }
    // The witness asserts the exact allowance/count before emitting these numbers.
    // Keep Chrome wall time separate: it includes waiting for 51 real refreshes.
    for line in text
        .lines()
        .filter(|s| s.contains("M8_METRIC") && s.contains("search_"))
    {
        println!("{line}");
    }
    Ok(())
}

/// Every measurement criterion reports independently, including hardware failures.
pub(super) fn run(output: &Result<String, String>, passed: &mut bool) {
    report(
        "M8.8 native search slice/total CPU and full 100000-line workload",
        native_search(),
        passed,
    );
    report(
        "M8.8 Chrome search slice/total CPU and wall time at DPR 1/2",
        browser_search(output),
        passed,
    );
    // Existing M8.2/browser witnesses enforce the same native/wasm scroll CPU limits.
    // Cadence must come from actual surface submission recordings, never that CPU probe.
    cadence::desktop_120(passed);
    report(
        "M8.8 all retained native 60 Hz scroll runs without dropped frames",
        cadence::desktop_60(),
        passed,
    );
    report(
        "M8.8 all retained Chrome 120 Hz scroll runs without dropped frames",
        cadence::chrome(),
        passed,
    );
}
