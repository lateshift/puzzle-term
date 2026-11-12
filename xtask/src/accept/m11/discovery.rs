//! Follow-up acceptance reproduces cold fallback pressure and actual PTY responsiveness.
//!
//! Native submissions are counted after successful surface presentation, and
//! child receipt is timed independently of the transport's enqueue clock.
//! The full Chrome suite checks this same producer at both DPRs in raster.rs.
//! Neither platform may accept a complete final grid as evidence of live frames.
//!
//! The controlled producer can remain stalled while can_draw and prepare return
//! provisional ink, then publish one exact whole-cluster result for swap-in.
//! A separate pressure witness exceeds the 4,096-entry allowance and requires
//! recent facts to survive. The tofu witness compares complete original contours,
//! so bounding atlas usage cannot silently remove diagnostic codepoint labels.
//! Native allocation measurements distinguish persistent index metadata from
//! transient font mappings, and negative lookups must allocate nothing.
//! Human appearance and comment quality remain pending for this follow-up;
//! the archived milestone verdict cannot certify newly changed behavior.
//! Repeated Unicode cycles additionally retain completed scalar coverage and
//! stable glyph IDs after temporary metadata pressure. A stationary 5,120-glyph
//! CJK frame must finish and rebuild without allocating or preparing replacement ink.
use super::{report, test};

/// Exact source witnesses separate lifetime bugs from Unicode presentation throughput.
const WITNESSES: &[(&str, &str, &str)] = &[
    (
        // Rotating CJK output resets VT at each refresh while source work completes.
        // All 5,120 glyphs must enter reusable atlas ink before the output ever stops.
        "changing 5120-glyph CJK screen retains completed ink",
        "puzzle-embed",
        "system_fonts::tests::activity::changing_cjk_screen_caches_completed_ink",
    ),
    (
        // A completed glyph must enter the shared cache after its original cell left.
        // This reproduces active-output loss independently of a stationary screen.
        "completed offscreen ink enters the atlas",
        "puzzle-term",
        "frame_builder::tests::fallback::completed_ink_is_cached_without_a_visible_cell",
    ),
    (
        // A stalled producer cannot block can_draw or prepare on the frame thread.
        // Its eventual complete result must replace the same provisional cluster.
        "cold source returns immediately and swaps completed ink",
        "puzzle-text",
        "deferred::tests::cold_source_returns_tofu_then_swaps_completed_ink",
    ),
    (
        // Entry retirement remains local: a later cluster retains its exact decision.
        // This guards against restoring the former wholesale-clear implementation.
        "pressure preserves recent cluster facts",
        "puzzle-text",
        "deferred::tests::pressure_keeps_recent_complete_cluster_facts",
    ),
    (
        // Repeated benchmark payloads revisit early scalars after 4,096 other choices.
        // Previously completed coverage must not become an unknown decision again.
        "Unicode cycles preserve every completed scalar fact",
        "puzzle-text",
        "deferred::tests::unicode_cycle_keeps_completed_scalar_facts",
    ),
    (
        // Stable scalar identities permit downstream atlas reuse without a UTF-8 owner.
        // Composed-cluster eviction must never renumber already prepared scalar ink.
        "Unicode source IDs survive composed metadata pressure",
        "puzzle-text",
        "font_set::presentation_tests::unicode_source_keys_survive_metadata_pressure",
    ),
    (
        // Sized work can outlive the temporary entry that originally submitted it.
        // Its producer-owned scalar key must still restore the correct ink owner.
        "late scalar ink survives temporary metadata retirement",
        "puzzle-text",
        "deferred::tests::late_scalar_ink_survives_metadata_retirement",
    ),
    (
        // This real installed-font screen exceeds both registry and mailbox capacity.
        // Completion and a zero-allocation full redraw prove bounded queues can converge.
        "stationary 5120-glyph CJK screen resolves and reuses warm ink",
        "puzzle-embed",
        "system_fonts::tests::flood::stationary_cjk_screen_finishes_fallback",
    ),
    (
        // Coverage facts are small; retaining all completed zoom bitmaps is not bounded.
        // Ready results retain their original independent eight-variant allowance.
        "completed zoom variants retain at most eight owned results",
        "puzzle-text",
        "deferred::tests::completed_zoom_variants_have_a_separate_retention_budget",
    ),
    (
        // Uncovered Unicode still needs a complete diagnostic hexadecimal label.
        // Shared border/digit geometry must match the original combined outline.
        "shared tofu preserves labels and bounds Unicode atlas growth",
        "puzzle-text",
        "builtin::tests::shared_tofu_preserves_complete_diagnostic_contours",
    ),
    (
        // File mappings and permanent candidate metadata have distinct memory costs.
        // Repeated indexed negative hits must allocate no per-query string or vector.
        "one-time index memory and allocation-free negative hits",
        "puzzle-embed",
        "system_fonts::tests::unicode_index_memory_and_negative_hits",
    ),
];

/// Runs deterministic source contracts before the isolated native-window flood.
pub(super) fn run(passed: &mut bool) {
    for &(label, crate_name, name) in WITNESSES {
        // Exact filters must run the named witness; zero tests cannot become a pass.
        report(
            &format!("M11 follow-up {label}"),
            test(crate_name, name),
            passed,
        );
    }
    report(
        "M11 follow-up native Unicode ≥30 fps, child key ≤50 ms, provisional/progressive ink",
        crate::unicode_flood::native(),
        passed,
    );
    // Earlier milestone reviews remain historical; these new behavior/comment checks are pending.
    println!("REVIEW M11 follow-up Unicode flood desktop/Chrome and comments: pending; README.md");
}
