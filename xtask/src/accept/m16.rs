//! Incremental desktop render-thread acceptance (D-41/D-43, R14/R15).
//!
//! New task criteria join this inventory with their implementations. Exact
//! witnesses reject zero-test success; goldens keep the reviewed references
//! unchanged while the headless harness transfers GPU ownership to the actor.
//! Passing implemented tasks cannot confirm a visual or hardware review.
//!
//! The full check exercises both native code and unchanged wasm contracts.
//! Golden execution uses the actor-backed offscreen harness, with the same
//! fonts, script dispatcher and GPU executor as before ownership transferred.
//! No reference is blessed and no pixel tolerance is relaxed in this gate.
//! Long-frame input uses a raw real PTY child that acknowledges encoded bytes.
//! Its build remains in progress when the child reports the key, proving the
//! input path does not sit behind shaping or a blocking surface acquisition.
//! Resize pressure tests mailbox coalescing and drops the native owner once.
//! Closing during that pressure must release the driver on the worker thread.
//! Device loss destroys a real GPU device, then compares replacement pixels.
//! Input metadata tests change focus and theme while a sealed frame is built.
//! The immutable snapshot must retain its old appearance, while later damage
//! survives on the Session owner for the next publication.
//! Pacing witnesses retain their injected clocks and original D-33/D-35 bounds.
//! Toolkit separation is checked in executable imports rather than comment prose.
//! A host may supply the safe surface from a native layer without a winit handle.
//! Pending user appearance and live-window reviews remain explicit REVIEW lines.
//! Subsequent tasks add their own complete-frame and measurement criteria here.
//! An implemented-task PASS never marks the whole milestone accepted.
//! Resize acceptance seals an old frame before replacing its attachment extent.
//! Exact red and background pixels distinguish top-left clipping from scaling,
//! including large jumps and a simultaneous change of device pixel ratio.
//! These tests use the ordinary frame executor, not a second resize renderer.
//! Wgpu’s actual validation callback is exercised with an invalid texture.
//! The product hook returns evidence and permits a later valid submission;
//! its strict counterpart must panic in the test process, preserving R14.
//! Native font failures are tested independently of GPU atlas eviction.
//! A cmap-positive empty outline becomes a failed candidate fact, not tofu
//! pretending to be coverage. Bounded traversal can finish over several queries.
//! Warm repeats must perform no further extraction or mapping, and individual
//! recency eviction must retain unrelated recently accessed negative facts.
use super::{m7::test, report};
/// D-47 adds bitmap identity, GPU-only recovery, warm budgets and maximized stress.
mod bitmap;
/// Complete native glyph preparation and release workload evidence.
pub(in crate::accept) mod complete;
/// D-53 retained native contexts and allocation-free refresh facts.
mod display_link;
/// Exposed-row allocation and CPU regressions supplement the live release recordings.
mod measurements;
/// D-50 ownership, publication distributions and actual writer-queue key budget.
mod owner;
/// D-51 lazy rows, pointer-sized region movement and both geometry recordings.
mod scrolling;
/// Incremental UAX29, exact grid ownership and stage-isolated throughput witnesses.
pub(in crate::accept) mod throughput;

/// Runs the implemented work-order tasks and leaves live quality with the reviewer.
pub(super) fn run(browser: super::BrowserGate) -> bool {
    let mut passed = true;
    // Native and wasm checks retain the original full definition-of-done gate.
    report(
        "native and wasm check",
        crate::commands::check(),
        &mut passed,
    );
    // The same predictor is invoked by RenderApp, without toolkit or input ownership.
    report(
        "M16.1 D-33/D-35 render-thread clock contracts",
        clocks(),
        &mut passed,
    );
    // The harness now executes scripts on the actor; no golden budget or pixel changes.
    report(
        "M16.1 all existing GPU/scenario goldens unchanged",
        crate::golden::run_all(false, false),
        &mut passed,
    );
    // Lifecycle regressions run in debug too; optimized timing cannot hide an exit hang.
    witnesses(&mut passed);
    // The host seam is intentionally extracted before creating puzzle-embed in M17.
    report(
        "M16.1 render actor/driver surface seam has no winit dependency",
        surface_seam(),
        &mut passed,
    );
    // U2 schedules real PTY/socket witnesses beside the Chrome closure gates.
    super::bridge::closure(browser, &mut passed);
    // Shared raster flags/uploads must keep the unchanged browser admission path green.
    browser.report(
        "M16 shared-code Chrome regression suite at DPR 1 and 2",
        || crate::browser::suite().map(|_| ()),
        &mut passed,
    );
    complete::run(&mut passed);
    // Cadence stalls remain recorded for the host-driven M17 refresh task, not hidden as PASS.
    measurements::run(&mut passed);
    bitmap::run(&mut passed);
    throughput::run(&mut passed);
    owner::run(&mut passed);
    display_link::run(&mut passed);
    scrolling::run(&mut passed);
    println!(
        "REVIEW M16.8 all ten matched vtebench reference benchmarks and smoothness, flood typing/Ctrl+C, resize and 120-Hz scrolling: user confirmed 2026-10-09 on f378fca; M16.4 bitmap cache accepted 2026-10-07; docs/history/M16.md"
    );
    // The user accepted these hardware/lifecycle outcomes; retain every raw run.
    println!(
        "REVIEW M16.8 controlled 120-Hz scrolling: display link 5/10, timer 0/10; all 20 runs retained; user accepted current 120-Hz state 2026-10-09, remaining misses M17.refresh. Idle link remains stopped by user decision; policy unchanged; docs/history/M16.md"
    );
    println!(
        "REVIEW M16 comment quality: actor, native input handoff, surface seam and recovery; pending reviewer; docs/history/M16.md"
    );
    println!(
        "REVIEW R08 M11 native follow-up comment quality/correctness: pending reviewer before M16 closure; docs/history/M11.md / docs/history/M16.md"
    );
    passed
}

/// M17 retains every native M16 criterion through the embedding-backed harness.
///
/// The parent supplies check and Chrome once; this inventory reuses the original
/// native bounds and reviewed pixels without relabeling M16's old review prose.
pub(super) fn embedding_regressions(passed: &mut bool) {
    // Clock policy and pixels precede workload timings, so a fast incorrect
    // renderer cannot substitute for the previous milestone's behavior.
    report(
        "M16.1 D-33/D-35 clock contracts through embedding",
        clocks(),
        passed,
    );
    report(
        "M16 goldens through embedding unchanged",
        crate::golden::run_all(false, false),
        passed,
    );
    witnesses(passed);
    report(
        "M16.1 toolkit-free embedding surface seam",
        surface_seam(),
        passed,
    );
    // Cold covered ink, reusable floods and real programs use actual PTYs.
    complete::run(passed);
    // Exposing history must retain its bound even when bitmap caches are warm.
    measurements::run(passed);
    bitmap::run(passed);
    // Parser and publication budgets isolate CPU work from FIFO acquisition.
    throughput::run(passed);
    owner::run(passed);
    // Retained callback storage and zero-cell scroll operations are structural
    // invariants; physical cadence evidence cannot establish either one.
    display_link::run(passed);
    scrolling::run(passed);
}

/// Independent actor and input witnesses; exact paths reject empty cargo filters.
const WITNESSES: [(&str, &str, &str); 14] = [
    // A published grid can outlive both its attachment size and device scale.
    (
        "M16.8 stale-size/DPR frames clip top-left exact GPU pixels",
        "puzzle-render",
        "frame_renderer::tests::resize_between_publish_and_acquire_keeps_top_left_pixels",
    ),
    // Product errors are observable without aborting and the next valid draw retains ink.
    (
        "M16.8 uncaptured validation error recovers without product panic",
        "puzzle-render",
        "frame_renderer::tests::uncaptured_error_is_observable_without_product_panic",
    ),
    // Strict diagnostics cannot certify a frame that recovered from validation.
    (
        "M16.8 strict GPU validation hook remains a test failure",
        "puzzle-render",
        "frame_renderer::tests::strict_uncaptured_error_remains_a_test_failure",
    ),
    // A cmap hit with no outline must stop reopening/shaping the same candidate.
    (
        "M16.8 failed native font candidate evaluated once",
        "puzzle-embed",
        "system_fonts::negative::tests::no_outline_candidate_is_evaluated_only_once",
    ),
    // Negative geometry keys must retain recency and individual eviction semantics.
    (
        "M16.8 negative candidate pressure retains recent decisions",
        "puzzle-embed",
        "system_fonts::negative::tests::eviction_keeps_recent_failures_without_clearing_the_working_set",
    ),
    // EOF can retain a final screen; shutdown cancels before render-owned sender drops.
    (
        "M16.7 held child output EOF and app shutdown join every worker",
        "puzzle-embed",
        "render_thread::tests::shutdown::held_child_shutdown_joins_all_threads",
    ),
    // Expensive rendering retains its endpoints while the Session must already stop.
    (
        "M16.7 held child shutdown joins during an occupied frame",
        "puzzle-embed",
        "render_thread::tests::shutdown::held_child_shutdown_during_frame_joins_all_threads",
    ),
    // A nested executor's thread parking cannot steal an outer actor notification.
    (
        "M16.7 nested render wait preserves actor wake",
        "puzzle-embed",
        "render_thread::tests::nested_executor_cannot_consume_actor_wake",
    ),
    // This witness encodes through the native adapter and waits for a real PTY child.
    (
        "M16.1 key reaches child ≤50 ms during 200-ms build",
        "puzzle-embed",
        "render_thread::tests::long_frame_does_not_delay_real_pty_key",
    ),
    // Thousands of output/drag changes stay bounded by one latest-size mailbox.
    (
        "M16.1 resize/output and close release surface on worker",
        "puzzle-embed",
        "render_thread::tests::resize_and_close_during_flood_release_on_worker",
    ),
    // Updates at the sleep boundary must not strand an idle worker or shutdown join.
    (
        "M16.1 idle wake retained and process shutdown joined",
        "puzzle-embed",
        "render_thread::tests::idle_wake_is_not_lost_and_shutdown_joins",
    ),
    // A destroyed real GPU device must restore exact pixels from CPU mirrors.
    (
        "M16.1 device loss restores ink on render thread",
        "puzzle-embed",
        "render_thread::tests::recovery::device_loss_restores_ink_on_render_thread",
    ),
    // Input geometry and focus reports remain accessible while row caches build.
    (
        "M16.1 focus and metrics survive detached frame",
        "puzzle-term",
        "session::tests::external_state::input_state_survives_external_build",
    ),
    // Theme changes during a build must invalidate later rows without altering the snapshot.
    (
        "M16.1 concurrent theme revision survives detached frame",
        "puzzle-term",
        "session::tests::external_state::theme_revision_survives_external_build",
    ),
];

/// Every witness runs even when another one failed, preserving R14’s complete report.
fn witnesses(passed: &mut bool) {
    for (label, package, name) in WITNESSES {
        // Neutral render tests need the same native backend already used by the harness.
        let result = if package == "puzzle-render" {
            gpu_witness(name)
        } else {
            test(package, name)
        };
        report(label, result, passed);
    }
}

/// Checks every existing clock witness, rather than choosing one passing example.
fn clocks() -> Result<(), String> {
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-embed",
            "window::pacing::tests",
            "--",
            "--nocapture",
        ],
    )?;
    // Cargo's empty filter exits successfully, so require the known clock namespace.
    if !output.contains("test window::pacing::tests::") {
        return Err("render-thread predictor clock witnesses did not execute".into());
    }
    Ok(())
}

/// The actor and renderer accept only copied host observations and safe GPU surfaces.
fn surface_seam() -> Result<(), String> {
    for path in [
        "crates/puzzle-embed/src/render_thread.rs",
        "crates/puzzle-embed/src/render_thread/host.rs",
        "crates/puzzle-embed/src/render_thread/surface.rs",
        "crates/puzzle-embed/src/window/driver.rs",
        "crates/puzzle-embed/src/window/driver/events.rs",
        "crates/puzzle-embed/src/window/driver/schedule.rs",
    ] {
        let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        // Comment prose may name the host toolkit; executable imports and paths may not.
        if source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .any(|line| line.contains("winit::"))
        {
            return Err(format!("{path} couples rendering to the native toolkit"));
        }
    }
    Ok(())
}

/// Exact GPU regressions use the native backend, while production wasm stays WebGPU-only.
fn gpu_witness(name: &str) -> Result<(), String> {
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-render",
            "--lib",
            "--features",
            "wgpu/default,wgpu/wgsl",
            name,
            "--",
            "--exact",
        ],
    )?;
    // should_panic is the strict-error proof; both forms must execute exactly one witness.
    if !output.contains(&format!("test {name} ")) || !output.contains("1 passed;") {
        return Err(format!("GPU witness {name} did not execute"));
    }
    Ok(())
}
