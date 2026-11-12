//! Incremental M8 acceptance inventory in the reviewer's task order.
//!
//! Each criterion executes its exact contract witness; an empty Cargo filter
//! is a failure. Later task criteria arrive with their implementations rather
//! than being reported as passing before the feature exists. The desktop STOP
//! still needs human review, independently of the mechanical checks.
//!
//! Following and anchoring share an output fixture but assert distinct states:
//! one ends at the new live tail, the other names the same tracked content.
//! The eviction witness deliberately shrinks history below that anchor.
//! Resize tests cover both row-count changes and physical-line remapping,
//! while screen transitions and reset test contexts with overlapping IDs.
//!
//! Scrollbar checks combine four sources in one document: shell status,
//! matches, a selection span and a bell. Navigation and track operations
//! must agree with thumb geometry rather than independently guessed offsets.
//! Frame-cache evidence distinguishes first exposure (one new layout) from
//! movement between prepared viewports (no reshaping of unchanged lines).
//!
//! The public Session input witness checks actual produced bytes. An inert
//! Super chord must leave history visible, whereas text and even an empty
//! paste follow under the default policy. Disabling onInput keeps the anchor.
//! Wire checks protect stable bus names and reject non-finite distances.
//! Browser tests use the real font-pack assets at both hinting paths, so a
//! correct grid cannot conceal a frame or atlas-upload defect.
//!
//! Motion witnesses are not wall-clock performance tests. The timeline tests
//! inject exact Instants and assert finite positions, silence deadlines and
//! cessation of frame requests. The separate CPU probe times actual assembly
//! into the reusable frame arrays, after real history and glyphs are prepared.
//! Every measured step exposes one previously unprepared full-width row.
//! Both clipped edge rows contribute to assembly without enlarging the target.
//! Chrome independently enforces the wasm one-row budget at both DPR settings.
//!
//! The native pacing test has a calibrated 120-Hz phase and an output burst
//! whose settle deadline lies after the next refresh. Animation must select
//! that earlier refresh while ordinary output remains deferred. The shared
//! rule test separately proves the exception does not rewrite the deadline.
//! The GPU fixture then asserts that this presentation geometry actually
//! clips all grid pipelines; candidate image comparison uses existing limits.
//!
//! M8.3 starts with synthetic winit pixel and line deltas, then executes the
//! actual public Wheel command through Session. Its mode table checks mouse
//! reporting precedence and Shift overrides independently of local selection
//! policy. Two half-row pixel samples must produce exactly one protocol step.
//! Literal arrow bytes pin DECCKM and prevent kitty negotiation changing 1007.
//! Keyboard defaults are executed for all three operating-system tables.
//! Prompt navigation replays a neutral OSC 133 recording through the parser;
//! repeated jumps must reach distinct retained line identities without input.
//! Selection stability is checked against a chosen historical substring.
//! Appended output and random width changes must preserve exactly that text.
//! A separate rewrite witness checks the opposite transition: the range clears.
//! The screen-epoch case switches away and back within one parser feed.
//! Copy cases distinguish soft continuity from explicit hard newlines.
//! They include a combining cluster, wide spacer and trim-disabled padding.
//! Semantic output tests ensure D at column zero excludes the next prompt.
//! Editing that prompt must leave the preceding completed output selected.
//! Gesture expansion checks a word split over the physical right margin.
//! A spacer click must paint/copy the entire two-cell character.
//! Wire fixtures exercise all operations, foreground-independent facts and invalid IDs.
//! Allocation measurements warm selection geometry before counting steady frames.
//! Readbacks cover all four visible kinds, both token choices and both DPR paths.
//! The fixture asserts pixels outside independently authored ranges are identical.

//! D-36 defers only native 120-Hz cadence; every retained repeat stays visible.
//! Chrome 120 Hz and native 60 Hz retain normal all-run acceptance.
//! Human confirmations below cite the user/reviewer dates, not GPU test results.

use super::{m7::test, report};
/// Pointer routing and clipboard representation witnesses belong to the desktop STOP.
mod input;
/// Actual search timings and recorded hardware presentation cadence.
pub(super) mod measurements;
/// Incremental literal queries and independent match paper readbacks.
mod search;
/// Real browser default ownership, complete subset API and submitted pixels.
mod web;
/// Runs source, viewport and frame-cache witnesses for the implemented tasks.
pub(super) fn run() -> bool {
    let mut passed = true;
    // The definition-of-done gate includes platform-neutral wasm compilation.
    report(
        "native and wasm check",
        crate::commands::check(),
        &mut passed,
    );
    viewport_contracts(&mut passed);
    animation_contracts(&mut passed);
    scrolling_contracts(&mut passed);
    selection_contracts(&mut passed);
    input::contracts(&mut passed);
    search::contracts(&mut passed);
    web::core(&mut passed);
    // The real-assets public-write guard prevents the M7 pack-upload regression
    // from being concealed by a viewport change that merely stores correct text.
    let output = crate::browser::suite();
    report(
        "Chrome complete suite at DPR 1 and 2",
        output.as_ref().map(|_| ()).map_err(Clone::clone),
        &mut passed,
    );
    web::browser(&output, &mut passed);
    measurements::run(&output, &mut passed);
    println!("REVIEW M8 desktop scrolling and selection: confirmed by user 2026-10-03; README.md");
    println!(
        "REVIEW M8 comment quality: M8.1–M8.5 confirmed Claude 2026-10-03; M8.6–M8.8 confirmed Claude 2026-10-04"
    );
    println!(
        "REVIEW M8 Chrome scrolling, selection/copy and search: confirmed by user 2026-10-04; README.md exact steps"
    );
    println!(
        "REVIEW M8 search references: assets/scenarios/search/*.png; confirmed by user 2026-10-04"
    );
    passed
}

/// Enforces the existing one-row CPU limit on newly exposed scrollback rows.
pub(super) fn scroll_budget() -> Result<(), String> {
    let output = crate::commands::capture(
        "cargo",
        &[
            "run",
            "--release",
            "-p",
            "puzzle-term",
            "--example",
            "scroll_bench",
        ],
    )?;
    // Retain exact raw probe output as reviewable evidence, not just a PASS label.
    std::fs::write("target/m8-scroll-native.txt", &output).map_err(|error| error.to_string())?;
    let millis: f64 = output
        .trim()
        .strip_prefix("scroll_ms=")
        .ok_or("scroll probe omitted measurement")?
        .parse()
        .map_err(|_| "invalid scroll measurement")?;
    if !millis.is_finite() || millis > 0.2 {
        return Err(format!("scroll frame {millis:.6} ms > 0.2"));
    }
    Ok(())
}

/// Document identity, following policy and marker geometry form the M8.1 gate.
fn viewport_contracts(passed: &mut bool) {
    for (label, name) in [
        (
            "M8.1 following, tracked output anchor and eviction clamp",
            "viewport::tests::following_anchor_and_eviction",
        ),
        ("M8.1 onOutput policy", "viewport::tests::output_policy"),
        (
            "M8.1 reflow/resize/screens/clears/RIS positions",
            "viewport::tests::transformations",
        ),
        (
            "M8.1 scrollbar geometry, all marker sources and model operations",
            "viewport::tests::scrollbar_geometry_markers_and_operations",
        ),
        (
            "M8.1 byte-producing input and paste following policy",
            "session::tests::viewport::input_following_and_viewport_facts",
        ),
        (
            "M8.1 whole-row cache movement rebuilds zero cached rows",
            "frame_builder::tests::viewport_cached_rows_never_reshape_for_movement",
        ),
        (
            "M8.1 typed navigation and viewport event wire validation",
            "messages::tests::viewport_wire_roundtrips",
        ),
    ] {
        // Exact-name execution protects the gate against accidental witness removal.
        report(label, test("puzzle-term", name), passed);
    }
}
/// Deterministic timelines and rendered edges form the M8.2 gate.
fn animation_contracts(passed: &mut bool) {
    for (label, name) in [
        (
            "M8.2 notch easing, continuous retarget and output anchor",
            "viewport::animation::tests::notch_retarget_and_anchor",
        ),
        (
            "M8.2 pixels, 150-ms silence and 100-ms snap",
            "viewport::animation::tests::pixel_snap_and_idle",
        ),
        (
            "M8.2 reduced motion and snapToRows false",
            "viewport::animation::tests::reduced_motion_and_no_snap",
        ),
        (
            "M8.2 Session idle stops frame requests",
            "session::tests::viewport::motion_session_stops_requesting_frames",
        ),
        (
            "M8.2 shared settle rule admits animation independently",
            "session::presentation::tests::animation_bypasses_output_settle",
        ),
    ] {
        // Timeline witnesses use injected Instants, never timing-sensitive sleeps.
        report(label, test("puzzle-term", name), passed);
    }
    report(
        "M8.2 desktop animation refresh bypasses output settle",
        test(
            "puzzle-embed",
            "window::pacing::tests::animation_keeps_refresh_despite_output_settle",
        ),
        passed,
    );
    report(
        "M8.2 fractional edge-row GPU/scissor/whole-pixel references",
        crate::golden::run_scroll(false, false),
        passed,
    );
    report(
        "M8.2 native 240x70 scroll frame CPU <=0.2 ms",
        scroll_budget(),
        passed,
    );
}

/// Native-shaped gestures and recorded prompts form the M8.3 gate.
fn scrolling_contracts(passed: &mut bool) {
    for (label, package, name) in [
        (
            "M8.3 native wheel routing table over tracking/1007/screen/DECCKM/Shift",
            "puzzle-desktop",
            "mouse::tests::scrolling::wheel_routing_table",
        ),
        (
            "M8.3 exact alternate-scroll keys and configured reset policy",
            "puzzle-desktop",
            "mouse::tests::scrolling::alternate_scroll_exact_keys_and_reset_policy",
        ),
        (
            "M8.3 scrolling actions and all platform defaults",
            "puzzle-term",
            "session::tests::scrolling::keyboard_scroll_actions_and_defaults",
        ),
        (
            "M8.3 recorded OSC 133 prompt jumps",
            "puzzle-term",
            "session::tests::scrolling::recorded_prompt_jumps",
        ),
    ] {
        // Each filter is exact and must execute a real witness, never zero tests.
        report(label, test(package, name), passed);
    }
}

/// Selection witnesses exercise model stability, Unicode copy and actual GPU cells.
fn selection_contracts(passed: &mut bool) {
    // Exact tests use public Session commands and reject empty filters.
    for (label, name) in [
        (
            "M8.4 selection survives output, scroll and reflow",
            "session::tests::selection::selection_stability",
        ),
        (
            "M8.4 random output/reflow preserves selected text",
            "session::tests::selection::selection_random_output_and_resizes",
        ),
        (
            "M8.4 rewrite, screen switch and RIS clear selection",
            "session::tests::selection::selection_clearing_rules",
        ),
        (
            "M8.4 copy wraps, hard breaks, trim, clusters and block rows",
            "session::tests::selection::selection_extraction_rules",
        ),
        (
            "M8.4 semantic newline excludes next prompt",
            "session::tests::selection::semantic_output_final_newline_and_same_row_boundaries",
        ),
        (
            "M8.4 wrapped words and wide spacer endpoints",
            "session::tests::selection::word_soft_wrap_and_wide_spacer",
        ),
        (
            "M8.4 selection actions and scrollbar markers",
            "session::tests::selection::selection_actions_and_markers",
        ),
        (
            "M8.4 Select/SelectionChanged wire validation",
            "messages::tests::selection::selection_wire_roundtrips_and_invalid_positions",
        ),
        (
            "M8.4 warm selected frame zero allocations and cache reuse",
            "frame_builder::tests::warm_selected_frame_allocates_nothing",
        ),
    ] {
        // Each label covers a distinct visible contract rather than test-count coverage.
        report(label, test("puzzle-term", name), passed);
    }
    // Cell ink, fills and unchanged outside pixels are asserted before comparison.
    report(
        "M8.4 four selection kinds, optional ink, DPR 1/2 GPU references",
        crate::golden::selection::run(false, false),
        passed,
    );
}
