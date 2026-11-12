//! Complete native-frame acceptance keeps every presentation and sampled GPU crop.
//!
//! These are release windows with the real PTY, adapter, message-driven session owner,
//! process render actor and ordinary FIFO executor. Compilation finishes before
//! observation starts. A parser grid or a final settling tail cannot pass here.
//! The legacy worst-frame-versus-refresh diagnostic remains in its JSON; D-41's
//! complete-frame gate follows D-45: complete ink, no aborted build, keys and memory.
use super::super::{m7::test, report};
use std::fs;
/// Existing local programs and neutral fixtures require no shipped asset changes.
mod fixtures;
/// Reusable-glyph producers share the same live actor and measurement interval.
mod flood;
/// Native windows retain raw reports and support explicit maximized comparison geometry.
mod window;

/// Runs complete-frame witnesses, actual benchmark and every real workload.
///
/// The deterministic tests create fresh native font owners and submit one image;
/// they cannot pass by letting a worker finish during a later settling frame.
/// The changing-screen witness also exercises reused row and glyph caches.
/// Capacity behavior is tested independently of native glyph discovery and live pacing.
fn raster_witnesses(passed: &mut bool) {
    report(
        "M16.2 synchronous raster eviction preserves current-frame ink",
        test(
            "puzzle-text",
            "raster::tests::synchronous_eviction_preserves_current_page_and_ordered_restore",
        ),
        passed,
    );
    // Exact pixels across several flushes catch uploads erasing earlier batches' ink.
    report(
        "M16.2 within-frame raster flushes preserve exact GPU pixels",
        test(
            "puzzle-desktop",
            "slug_probe::pressure_tests::raster::within_frame_flushes_preserve_exact_pixels_and_final_residency",
        ),
        passed,
    );
    // Both atlas limits must compose: a vector rejection may reach a referenced full raster.
    report(
        "M16.2 vector overflow into full raster retains covered ink",
        test(
            "puzzle-text",
            "glyph::slug::tests::vector_overflow_into_full_raster_retains_both_kinds_of_ink",
        ),
        passed,
    );
}

/// Static inventory guards against leaving an obsolete desktop owner reachable.
/// Live pressure then exercises the real PTY and FIFO surface in release mode.
/// All criteria report independently, matching the R14 continue-after-failure rule.
/// A failed release build prevents measurements from using an older executable.
/// The existing M16.1 acceptance remains responsible for unchanged goldens,
/// input concurrency, resize, lifecycle and device-loss behavior.
pub(super) fn run(passed: &mut bool) {
    // Keep both unrelated CJK outlines and composed Hangul in the cold GPU cohort.
    for (label, name) in [
        (
            "M16.2 cold CJK first-submission GPU ink",
            "system_fonts::tests::flood::stationary_cjk_screen_finishes_fallback",
        ),
        (
            "M16.2 5,120 Hangul first-submission GPU ink",
            "system_fonts::tests::flood::frozen_hangul_first_frame_has_complete_gpu_ink",
        ),
        (
            "M16.2 every changing CJK frame complete without queued glyphs",
            "system_fonts::tests::activity::changing_cjk_screen_caches_completed_ink",
        ),
    ] {
        // Exact filters cannot succeed from zero tests or renamed witnesses.
        report(label, test("puzzle-embed", name), passed);
    }
    raster_witnesses(passed);
    // Deleted owners are checked separately from the first-frame behavior proof.
    report(
        "M16.2 desktop admission, glyph lane and provisional source path removed",
        removed(),
        passed,
    );
    // Optimized code is built before any timing window opens.
    let built = release();
    // Build failure cannot let an older executable supply passing measurements.
    if let Err(error) = built {
        report("M16.2 release measurement binary", Err(error), passed);
        return;
    }
    // The pressure fixture uses the full sibling benchmark, not a reduced corpus.
    report(
        "M16.2 actual vtebench complete frames/GPU ink and child key ≤50 ms",
        unicode(),
        passed,
    );
    report(
        "M16.2 reusable-glyph Flood ≥30 fps on the render thread",
        flood::run(),
        passed,
    );
    // Real app output exercises family, style, bitmap and cluster combinations.
    report(
        "M16.2 eight real workloads have ink in first presented frame",
        workloads(),
        passed,
    );
}

/// Static inventory complements behavior tests of both cold and changing screens.
///
/// These paths owned the removed admission policy, pending jobs and completion
/// deadlines. The remaining native source, event driver and pacing module must
/// contain no executable admission or retargeting branch. Prose may explain the
/// removal, so comments are excluded from the token inventory. Shared browser
/// continuations deliberately remain outside this desktop-only check.
fn removed() -> Result<(), String> {
    // A renamed call site cannot excuse a still-present obsolete owner module.
    for old in [
        "system_fonts/worker.rs",
        "system_fonts/worker/admission.rs",
        "system_fonts/worker/loaded.rs",
        "system_fonts/worker/state.rs",
        "window/pacing/completion.rs",
    ] {
        // Check the workspace source paths, independent of generated target artifacts.
        if ["puzzle-desktop", "puzzle-embed"].iter().any(|owner| {
            std::path::Path::new("crates")
                .join(owner)
                .join("src")
                .join(old)
                .exists()
        }) {
            return Err(format!("desktop glyph-admission owner remains: {old}"));
        }
    }
    // These are the complete source and its scheduling/presentation entry points.
    for path in [
        "crates/puzzle-embed/src/system_fonts/synchronous.rs",
        "crates/puzzle-embed/src/window/driver/events.rs",
        "crates/puzzle-embed/src/window/pacing.rs",
    ] {
        // Read failure means the source was not audited and cannot pass the criterion.
        let source = fs::read_to_string(path).map_err(|e| e.to_string())?;
        // Exclude prose references; only executable uses could defer covered glyph work.
        if source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .any(|line| {
                // Pending source results would permit a covered cell to show a box.
                // Budget and retarget tokens represent the discarded admission policy.
                [
                    "Deferred",
                    "ClusterGlyph::Pending",
                    "set_completion_budget",
                    "completion_budget",
                    "retarget",
                    "ordinary_work",
                ]
                .iter()
                .any(|word| line.contains(word))
            })
        {
            return Err(format!("{path} still admits or defers desktop glyphs"));
        }
    }
    Ok(())
}

/// Finishes compilation before collecting release-only live measurements.
///
/// Lock metrics are existing optional diagnostic instrumentation, not a changed
/// threading policy. The measured child receives the ordinary release binary.
/// No debug profile or simultaneous compilation can supply a timing verdict.
/// Compiler output is preserved by the acceptance command on failure.
pub(in crate::accept) fn release() -> Result<(), String> {
    // Build the same native program that users launch through the harness command.
    crate::commands::quiet(
        "cargo",
        &[
            "build",
            "--release",
            "-p",
            "puzzle-desktop",
            "--features",
            "lock-metrics",
        ],
    )
}

/// Actual thirty-second benchmark plus the first frozen Hangul frame.
///
/// The producer retains the complete thirty-second vtebench output and then
/// displays exactly 5,120 Hangul syllables. Readiness is required in the first
/// successful frozen submission, before the thirty-second unchanged tail.
/// CPU readiness covers every pressure presentation; GPU crops sample its first
/// nonempty image and a deterministic two-second cadence. Every independently
/// timed key must reach the child within 50 ms, including late pressure keys.
/// Raw frame attempts and the legacy worst-frame diagnostic remain in JSON.
fn unicode() -> Result<(), String> {
    // Resolve existing benchmark resources before starting the diagnostic window.
    let script = fixtures::pressure()?;
    let value = window(&script, None, true, "unicode-pressure")?;
    // D-45 requires complete ink and zero aborted builds; fps is recorded without a floor.
    // Vector capacity is checked independently of raster and mapped-font accounting.
    if value["unicode_stress_pass"] != true
        || value["vector_bytes"]
            .as_u64()
            .is_none_or(|bytes| bytes > 48 * 1024 * 1024)
    {
        return Err(format!(
            "Unicode stress failed: fps={} key_ms={}",
            value["fps"], value["key_to_child_ms"]
        ));
    }
    // The first ordinal is authoritative; never search for a later ready frame.
    let first = value["frozen_presentations"]
        .as_array()
        .and_then(|frames| frames.first())
        .ok_or("first frozen presentation absent")?;
    // Both count and readiness prevent a clipped or partially populated screen passing.
    // The final GPU crop adds an executor-level check of the unchanged retained image.
    if first["cells"] != 5120 || first["ready"] != 5120 || value["frozen"]["pass"] != true {
        return Err("first frozen Hangul frame lacks complete covered ink".into());
    }
    // Print only a compact summary; all raw costs and sampled cells stay in the artifact.
    println!(
        "M16 Unicode fps={} worst_ms={} key_ms={}; covered sampled GPU cells missing=0",
        value["fps"], value["worst_frame_ms"], value["key_to_child_ms"]
    );
    Ok(())
}

/// All eight workloads run even if an earlier one failed; every appearance must be first-frame.
///
/// These are the existing authorized neutral fixtures: three language files,
/// a CJK/emoji directory, installed nvim and tmux, an emoji-rich chat log, and
/// a local mandoc page. Each run starts a fresh context and launches a real child.
/// A latency of zero means the first submitted appearance already has final ink;
/// it does not claim zero producer-to-scanout time. Unsupported scalars are excluded
/// by builtin identity, while covered provisional requests remain in the series.
/// Unresolved or censored appearances fail, including ones overwritten by tmux.
/// Every sampled GPU image must pass, rather than only the final settling image.
fn workloads() -> Result<(), String> {
    // A missing local producer is an explicit failure, never synthetic app evidence.
    let script = fixtures::workloads()?;
    // Aggregate failures so a broken earlier workload cannot hide later regressions.
    let mut failures = Vec::new();
    for name in [
        "cat-zh", "cat-ja", "cat-ko", "ls", "nvim", "tmux", "chat", "man",
    ] {
        // Idle time is intentional here; flood fps is assessed by the real benchmark above.
        let result = window(&script, Some(name), false, name).and_then(|value| {
            // Inspect full appearance history, not only the currently visible requests.
            let ink = &value["ink_appearances"];
            // Empty evidence cannot establish that any covered glyph actually drew.
            if ink["covered_appearances"]
                .as_u64()
                .is_none_or(|count| count == 0)
                || ink["max_ms"] != 0.0
                || ink["unresolved_visible"] != 0
                || ink["censored"] != 0
                // A final image supplements rather than replaces sampled first-frame ink.
                || value["retained_gpu"]["pass"] != true
                || value["gpu_samples"].as_array().is_none_or(|samples| {
                    samples.is_empty() || samples.iter().any(|sample| sample["pass"] != true)
                })
                || ink["missing_per_frame"].as_array().is_none_or(|frames| {
                    frames.is_empty() || frames.iter().any(|missing| *missing != 0)
                })
                || value["key_to_child_ms"].as_f64().is_none_or(|ms| ms > 50.0)
            {
                return Err(format!("{name}: first-frame/GPU/key evidence failed"));
            }
            // Preserve every per-app result in addition to the combined acceptance line.
            println!(
                "M16 {name}: first-frame ink max_ms={} child key_ms={}",
                ink["max_ms"], value["key_to_child_ms"]
            );
            Ok(())
        });
        // Continue the cohort regardless of individual process or evidence failure.
        if let Err(error) = result {
            failures.push(error);
        }
    }
    // Only a fully passing eight-workload cohort satisfies this criterion.
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

/// Keeps existing workload call sites on their original fixed-size geometry.
pub(super) fn window(
    script: &str,
    workload: Option<&str>,
    pressure: bool,
    label: &str,
) -> Result<serde_json::Value, String> {
    window::run(script, workload, pressure, label, false)
}

/// Runs the full Unicode corpus at the user's maximized comparison size.
pub(in crate::accept) fn maximized_unicode() -> Result<serde_json::Value, String> {
    // This helper is also called independently of complete::run. Its earlier
    // build failure must never fall through to a stale non-observing executable.
    release()?;
    let script = fixtures::pressure()?;
    window::run(&script, None, true, "bitmap-maximized", true)
}

/// Ordinary producing cadence is measured independently of periodic GPU ink reads.
pub(in crate::accept) fn maximized_cadence() -> Result<serde_json::Value, String> {
    let script = fixtures::pressure()?;
    window::cadence(&script)
}
