//! M16.5 parser equivalence, allocation discipline and measured pipeline stages.
//! The complete live/GPU acceptance stays in M16.2/M16.4. These lines add the
//! actual Unicode corpus differential and its incremental-boundary oracle, plus
//! finite real-PTY and headless measurements. The four user-approved §15 rows
//! bound Unicode, tmux and warm full-screen work independently of publication timing.
//!
//! The parser and renderer share bytes, but do not share grapheme ownership:
//! VT cells remain the source of column spans after cursor edits and wrapping.
//! A trailing Prepend mark reproduces the old out-of-grid glyph independently
//! of PTY handoff and session-thread scheduling. It accompanies optimization so
//! faster ingestion cannot conceal a drawing defect behind a good byte rate.
//!
//! The boundary oracle is the approved segmentation crate's own conformance
//! data, read from Cargo's resolved source rather than copied into our assets.
//! The grid oracle is the old string-based Terminal implementation, test-only.
//! It compares actual cells and D-31 widths after each offered byte slice.
//! Keeping both oracles catches a correct UAX machine with incorrect VT width
//! behavior, and a correct narrow corpus implementation with missing GB rules.
//!
//! Headless timing calls VT and Session directly; the PTY stage owns a real
//! child, the fixed pool and the production parser, with no renderer attached.
//! Frame-request publication is measured separately using that same Session owner.
//! Setup, compilation and artifact serialization never enter parser clocks.
//! Five chronological trials retain cold growth and scheduler variation; this
//! command does not choose a fastest sample or infer throughput from a frame.
//!
//! Full-window frame work is observed by the earlier bitmap acceptance run.
//! Its periodic GPU readbacks establish covered ink, but perturb cadence.
//! A separately labelled timing-only report cannot replace that ink witness.
//! Counters are cumulative at each frame, allowing a later evidence reader to
//! subtract the same warm interval for every cache layer and preparation site.
//! Raster misses can mean a deliberately smaller vector, rather than eviction.
//! Native coverage positives and negatives are immutable-index eligibility
//! observations, distinct from the string-keyed shaping and source registries.
//! These distinctions prevent unchanged cache population from concealing churn.
/// User-approved byte-rate and refresh-derived frame ceilings.
pub(in crate::accept) mod budgets;
use super::super::{m7::test, report};
use std::{fs, path::PathBuf, process::Command};

/// Executes each independent behavior before reporting throughput observations.
/// All tests use their exact names and a nonempty-test witness from m7::test.
/// Reporting continues after a failure, so no fast path hides the later GPU
/// or actual-corpus contracts. Visual parity remains a user review item.
pub(super) fn run(passed: &mut bool) {
    cache_behavior_tests(passed);
    behavior_tests(passed);
    report(
        "M16.5 actual Unicode corpus differential and zero warm allocations",
        differential(),
        passed,
    );
    report(
        "M16.5 headless and real-PTY stages measured without renderer",
        stages(),
        passed,
    );
    budgets::run(passed);
    report(
        "M16.5 warm cache-layer and full-new-Unicode-frame observations",
        warm_frames(),
        passed,
    );
}

/// Cache regressions independently preserve damage scheduling and bounded negatives.
fn cache_behavior_tests(passed: &mut bool) {
    for (label, package, name) in [
        (
            // UI delivery cannot repeatedly rearm damage before the scheduled snapshot.
            "M16.5 output wake coalescing preserves final write and D-33 burst timing",
            "puzzle-term",
            "session::tests::publication_wake::host_delivery_keeps_latch_but_publication_rearms_final_output",
        ),
        (
            // Completed cmap-positive failures must not reopen mappings on every frame.
            "M16.5 completed glyph negatives preserve warm allocation and policy changes",
            "puzzle-text",
            "font_set::tests::synchronous::completed_missing_retries_only_for_changed_identity_or_policy",
        ),
        (
            // Full metadata storage replaces one cold result rather than clearing facts.
            "M16.5 negative validation pressure preserves recent results",
            "puzzle-text",
            "font_set::sources::missing::tests::capacity_retains_recent_results_and_warm_lookup_allocates_nothing",
        ),
    ] {
        // Each exact test protects the host's warm working set or late snapshot wake.
        report(label, test(package, name), passed);
    }
}

/// Reports the independently named regressions before collecting pipeline measurements.
/// Keeping these pure witnesses separate from measurement orchestration preserves
/// R7's function limit without changing ordering or ignoring a failed test.
fn behavior_tests(passed: &mut bool) {
    for (label, package, name) in [
        (
            // Conjunct, emoji-ZWJ and RI suffixes require more than a pair rule.
            "M16.5 Unicode 17 incremental UAX29 conformance",
            "puzzle-vt",
            "terminal::print::boundary::tests::unicode_17_conformance_matches_current_segmentation",
        ),
        (
            // D-31 selector widths are terminal policy, beyond UAX boundaries.
            "M16.5 conformance cells and D-31 widths match original parser",
            "puzzle-vt",
            "terminal::print::boundary::tests::conformance_grid_and_widths_match_original_terminal",
        ),
        (
            // VT's zero-width attachment can differ from fresh string segmentation.
            "M16.5 grid preserves trailing Prepend cell at delayed wrap",
            "puzzle-term",
            "frame_builder::tests::cell_ownership::trailing_prepend_cluster_cannot_create_an_out_of_grid_glyph",
        ),
        (
            // Byte capacity must recycle even before the configured line ceiling.
            "M16.5 sparse history accounting and zero warm allocation",
            "puzzle-vt",
            "ring::tests::sparse_byte_capacity_recycles_and_resize_recounts",
        ),
        (
            // Full and partial deque fragments preserve single-step identities.
            "M16.5 batched scroll regions preserve identity without allocation",
            "puzzle-vt",
            "ring::tests::region_counts_match_single_steps_without_allocation",
        ),
        (
            // BCE clears outside partners when an erase splits both wide borders.
            "M16.5 bulk erase preserves wide borders and reusable cluster storage",
            "puzzle-vt",
            "row::tests::bulk_erase_retires_both_wide_borders_and_reuses_cluster_storage",
        ),
        (
            // Nearest-only masks must use budget bytes without interpolation waste.
            "M16.5 nearest grid crops use full byte budget without eviction",
            "puzzle-text",
            "raster::tests::grid_crops_use_full_page_area_without_eviction",
        ),
    ] {
        // A successful empty cargo filter cannot certify any of these contracts.
        report(label, test(package, name), passed);
    }
}

/// Resolves the user's installed sibling corpus; nothing is fetched or vendored.
/// An explicit environment path supports another checkout without changing bytes.
/// Missing data is a failed criterion, never a tiny replacement corpus presented
/// as a benchmark result. Parent resolution also works from the workspace root.
fn corpus() -> Result<PathBuf, String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    // PathBuf arguments avoid shell expansion of names supplied by a host.
    let path = std::env::var_os("PUZZLE_UNICODE_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            root.parent()
                .unwrap_or(&root)
                .join("vtebench/benchmarks/unicode/symbols")
        });
    if !path.is_file() {
        return Err("installed vtebench Unicode corpus absent".into());
    }
    Ok(path)
}

/// Runs the independent old parser over full corpus bytes, including split UTF-8.
/// Ordinary workspace tests may use a small checked-in Unicode fixture. This
/// criterion explicitly supplies the complete installed corpus and proves its
/// two named differential/allocation tests ran. Exit-zero alone is insufficient.
fn differential() -> Result<(), String> {
    let output = Command::new("cargo")
        .args([
            "test",
            "-p",
            "puzzle-vt",
            "terminal::print::boundary::tests",
            "--",
            "--test-threads=1",
        ])
        // The allocation probe warms a complete row-ring cycle on actual bytes.
        .env("PUZZLE_UNICODE_CORPUS", corpus()?)
        .output()
        .map_err(|e| e.to_string())?;
    // Preserve failed cold and warm measurements instead of replacing them with a verdict.
    fs::create_dir_all("target/m16-5").map_err(|e| e.to_string())?;
    let mut log = output.stdout.clone();
    log.extend_from_slice(&output.stderr);
    fs::write("target/m16-5/accept-differential.log", &log).map_err(|e| e.to_string())?;
    // Cargo's test names are ASCII even when diagnostics contain Unicode cells.
    let log = String::from_utf8_lossy(&log);
    if !output.status.success()
        || !log.contains("unicode_corpus_matches_original_terminal ... ok")
        || !log.contains("warm_unicode_corpus_allocates_nothing ... ok")
    {
        return Err("actual-corpus differential/warm allocation test failed; target/m16-5/accept-differential.log".into());
    }
    Ok(())
}

/// Keeps compilation outside native measurement and validates exact PTY consumption.
/// The reusable example emits sixty samples: two owners, two width policies,
/// three slice sizes and five trials. The PTY witness independently checks the
/// offered-byte count and pending suffix, rather than trusting a timer report.
/// Measurement artifacts are local evidence and never enter the distributed app.
fn stages() -> Result<(), String> {
    // Resolve once so both probes consume the identical externally supplied stream.
    let path = corpus()?;
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
    // The example times five chronological VT/Session trials per width policy and slice size.
    let status = Command::new("target/release/examples/throughput_bench")
        .arg(&path)
        .arg("target/m16-5/accept-headless.csv")
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("headless stage probe failed".into());
    }
    // The header plus all sixty observations form a complete chronological report.
    let csv = fs::read_to_string("target/m16-5/accept-headless.csv").map_err(|e| e.to_string())?;
    if csv.lines().count() != 61 {
        return Err("headless chronological samples absent".into());
    }
    pty_stages(&path)?;
    println!(
        "M16.5 stage observations: target/m16-5/accept-headless.csv and pty-stages.json; approved floors checked separately"
    );
    Ok(())
}

/// Drives the independent production PTY seam after the headless probe ends.
/// The ignored release test retains all ten cohorts and verifies exact bytes.
/// Captured output is serialized after each test, outside the stage clocks.
fn pty_stages(path: &std::path::Path) -> Result<(), String> {
    // The PTY test verifies exact offered bytes, empty suffix and the fixed 4-MiB pool.
    let output = Command::new("cargo")
        .args([
            "test",
            "--release",
            "-p",
            "puzzle-embed",
            "--features",
            "lock-metrics",
            "session_thread::tests::stages::unicode_pty_stage_measurements",
            "--",
            "--exact",
            "--ignored",
        ])
        // --ignored selects this finite measurement, not ordinary default tests.
        .env("PUZZLE_UNICODE_CORPUS", path)
        .output()
        .map_err(|e| e.to_string())?;
    // Join both output streams only after timing, preserving failed setup reasons.
    let mut log = output.stdout.clone();
    log.extend_from_slice(&output.stderr);
    fs::write("target/m16-5/accept-pty.log", &log).map_err(|e| e.to_string())?;
    if !output.status.success()
        || !String::from_utf8_lossy(&log)
            .contains("test session_thread::tests::stages::unicode_pty_stage_measurements ... ok")
    {
        return Err("real-PTY stage probe failed; target/m16-5/accept-pty.log".into());
    }
    Ok(())
}

/// Reads the fresh maximized ink run generated earlier in this same acceptance.
/// No stale manual report can supply this criterion: bitmap::run launches and
/// stores its window first. The warm interval follows initial corpus discovery.
/// Every cache count remains integer-valued, and every frame has its own build
/// and row phases. This validates observation completeness without inventing
/// a new latency threshold before the user's requested budget discussion.
fn warm_frames() -> Result<(), String> {
    let bytes = fs::read("target/m16-2/bitmap-maximized.json").map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    // A timing-only cohort cannot satisfy the ink requirement accidentally.
    if value["unicode_stress_pass"] != true || value["cadence_only"] == true {
        return Err("fresh matched-size GPU ink cohort failed".into());
    }
    // Only actual presented builds participate; retries remain in the original JSON.
    let attempts = value["frame_gates"]["attempts"]
        .as_array()
        .ok_or("frame chronology absent")?;
    // Selecting by timestamp keeps frame-count differences between cohorts visible.
    let warm: Vec<_> = attempts
        .iter()
        .filter(|frame| {
            frame["outcome"] == "presented"
                && frame["start_ms"]
                    .as_f64()
                    .is_some_and(|t| (10_000.0..30_000.0).contains(&t))
        })
        .collect();
    if warm.is_empty() {
        return Err("warm producing frames absent".into());
    }
    // A partially instrumented late frame is a failure even if earlier rows were complete.
    for frame in &warm {
        // Six independently named layers contain six exact scalar counts each.
        let layers = frame["cache_metrics"]
            .as_array()
            .ok_or("cache-layer observations absent")?;
        if layers.len() != 6
            || layers.iter().any(|layer| {
                layer.as_array().is_none_or(|counts| {
                    counts.len() != 6 || counts.iter().any(|count| count.as_u64().is_none())
                })
            })
        {
            return Err("incomplete warm cache-layer observations".into());
        }
        // Build and row stages stay separate from upload/acquire and observer overhead.
        if frame["row_phases_ms"]
            .as_array()
            .is_none_or(|phases| phases.len() != 4)
            || frame["build_ms"].as_f64().is_none()
        {
            return Err("row phase observations absent".into());
        }
    }
    println!(
        "M16.5 matched-size warm observations: {} frames; target/m16-2/bitmap-maximized.json; visual cadence requires user review",
        warm.len()
    );
    Ok(())
}
