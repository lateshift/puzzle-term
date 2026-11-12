//! M8.6 bounded search, retained navigation, wire policy and GPU fills.
//!
//! Large history and giant soft wraps exercise independent sources of unbounded
//! work. Output witnesses distinguish append, eviction, rewrite and screen switch.
//! The query cap and physical-row allowance are independent constraints.
//! A short query may reach the cap early, while a sparse query scans the tail.
//! The 100,000-line witness tests convergence, partial counts and idle cessation.
//! A giant wrapped line tests bounded overlap memory and backward discovery.
//! A gone frontier tests that overlap text cannot outlive its tracked row.
//!
//! Smart case includes both lowercase and uppercase queries; hard boundaries
//! must remain distinct from WRAPPED continuation. Wide clusters occupy two
//! paper cells even though their searchable text has one owning position.
//! Live output preserves unaffected hits, but rewrites discard the logical suffix.
//! Navigation then checks the selected index, centred line and marker sources.
//!
//! Platform action tests protect desktop passthrough while its bar is deferred.
//! The web request remains a coarse UI fact without scanning the query inline.
//! Wire tests reject regex and mistyped case options before state mutation.
//! Theme tests pin explicit search paper colors independently of derived defaults.
//! Warm assembly proves paper changes do not reshape or allocate glyph data.
//!
//! Exact-name execution rejects removed tests rather than accepting empty filters.
use super::{report, test};
/// Appends search criteria after the completed desktop-selection tasks.
pub(super) fn contracts(passed: &mut bool) {
    for (label, package, name) in [
        (
            "M8.6 100000 lines converge over <=2000-row frame slices",
            "puzzle-term",
            "session::tests::search::hundred_thousand_lines_are_sliced",
        ),
        (
            "M8.6 giant soft wrap bounds forward and rewind work",
            "puzzle-vt",
            "search::tests::giant_soft_wrap_has_bounded_scratch_and_rewind",
        ),
        (
            "M8.6 evicted frontier cannot invent overlap hits",
            "puzzle-vt",
            "search::tests::evicted_frontier_cannot_invent_a_cross_boundary_match",
        ),
        (
            "M8.6 smart case, soft wraps, hard breaks and wide cells",
            "puzzle-term",
            "session::tests::search::smart_case_wraps_and_wide_cells",
        ),
        (
            "M8.6 append, eviction, rewrite and screen results",
            "puzzle-term",
            "session::tests::search::output_eviction_and_rewrite_reconcile_matches",
        ),
        (
            "M8.6 current navigation centres and updates markers",
            "puzzle-term",
            "session::tests::search::navigation_centres_and_updates_markers",
        ),
        (
            "M8.6 platform search request and performable next/prev",
            "puzzle-term",
            "session::tests::search::search_actions_follow_platform_and_result_availability",
        ),
        (
            "M8.6 literal search wire validation rejects regex",
            "puzzle-term",
            "messages::tests::search::search_wire_roundtrips_and_rejects_unknown_options",
        ),
        (
            "M8.6 authored search colors and invalid token rejection",
            "puzzle-term",
            "theme::tests::search_tokens_are_authored_or_derived",
        ),
        (
            "M8.6 warm search fills allocate zero and preserve glyph caches",
            "puzzle-term",
            "frame_builder::tests::warm_search_frame_allocates_nothing",
        ),
    ] {
        // Each witness protects a distinct user-visible contract or hot-path rule.
        report(label, test(package, name), passed);
    }
    // Count/current and per-cell ink/paper checks precede unchanged golden comparison.
    report(
        "M8.6 match/current fills DPR 1/2 GPU references",
        crate::golden::search::run(false, false),
        passed,
    );
}
