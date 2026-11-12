//! M15.6 checks protocol cells separately from their actual GPU tile pixels.
//!
//! The table digest pins kitty's complete list, including supplementary scalars.
//! Session tests use sealed publications, not renderer access to live source state.
//! Real-client appearance remains a user review item even after these witnesses pass.
//!
//! Each exact test name is a separate criterion so a protocol regression cannot
//! disappear inside a successful rendering summary. The shared report helper
//! retains failures and continues through the remaining witnesses (R14).
//! The task selector runs just this block; full M15 acceptance also runs its
//! unchanged throughput/frame budgets and Chrome before milestone closure.
//! Generated PNGs use authored pixels, while the table is pinned to kitty's
//! published wire order rather than the Unicode properties of today's release.
//! DPR-specific readbacks exercise actual raster submissions and device recovery.
//!
//! Driver selection and drawing have independent failure modes. The native probe
//! witness includes asynchronous decode and DA1 ordering; Session witnesses cover
//! the placement table after that selection has already happened. Golden readbacks
//! then compare image pixels, rather than treating successful protocol replies as
//! proof of visible output. This separation also preserves useful failure reports
//! when a quiet client suppresses placement errors on the wire.
use crate::accept::{m7, report};
/// Every placeholder requirement has an exact named witness in the final M15 gate.
fn protocol(passed: &mut bool) {
    for (criterion, test) in [
        // A complete independent digest catches substitutions, omissions and reordering.
        (
            "M15.6 exact 297-entry protocol table",
            "protocols::kitty::graphics::placeholders::tests::exact_protocol_diacritic_table",
        ),
        // Resolved palette RGB must never replace raw indexed or truecolor identifiers.
        (
            "M15.6 raw indexed/RGB and high-byte IDs",
            "protocols::kitty::graphics::placeholders::tests::raw_colour_ids_and_high_byte",
        ),
        // All 256 palette indices, including ANSI aliases, retain their wire payload.
        (
            "M15.6 all 256 indexed IDs and ANSI aliases",
            "protocols::kitty::graphics::placeholders::tests::all_indexed_image_and_placement_ids_are_raw",
        ),
        // Omitted fields inherit only across the three protocol-defined left-cell cases.
        (
            "M15.6 all left-inheritance rules and boundaries",
            "protocols::kitty::graphics::placeholders::tests::inheritance_resets_on_colour_text_and_row_boundaries",
        ),
        // Both creation actions are invisible prototypes and must preserve the cursor.
        (
            "M15.6 invisible virtual transmit/put and stationary cursor",
            "protocols::kitty::graphics::placeholders::tests::virtual_placements_do_not_draw_or_move_cursor",
        ),
        // Physical selectors cannot remove a prototype with no screen position.
        (
            "M15.6 virtual lifetime, deletes and visible admission",
            "protocols::kitty::graphics::placeholders::tests::virtual_deletion_and_visible_admission",
        ),
        // Publication owns immutable pixels; adjacent tiles share one instanced draw.
        (
            "M15.6 sealed tiles, batched draws and warm zero allocations",
            "session::tests::images::placeholders::virtual_tiles_are_sealed_batched_and_never_glyphs",
        ),
        // Disabled/unknown metadata still bypasses font fallback, while paper paints.
        (
            "M15.6 unknown/disabled placeholders never prepare tofu",
            "session::tests::images::placeholders::unknown_placeholders_never_prepare_tofu",
        ),
        // Known images still draw when every reserved SGR decoration is enabled.
        (
            "M15.6 indexed/RGB tiles suppress every text decoration",
            "session::tests::images::placeholders::known_placeholder_ids_ignore_all_text_decorations",
        ),
        // VT editing changes tile locations, not their prototype-relative UV coordinates.
        (
            "M15.6 tiles follow scrolling, reflow and clear",
            "session::tests::images::placeholders::placeholder_tiles_follow_reflow_scroll_and_clear",
        ),
    ] {
        // An absent/renamed test fails; Cargo's empty-filter success is insufficient.
        report(criterion, m7::test("puzzle-term", test), passed);
    }
}

/// Completes the protocol receipt with actual child identity and GPU pixels.
pub(super) fn run(passed: &mut bool) {
    protocol(passed);
    yazi(passed);
    // Shared child spawn cannot inherit a different terminal's adapter identity.
    report(
        "M15.6 clean native child terminal identity",
        m7::test(
            "puzzle-embed",
            "pty::tests::identity::terminal_identity_removes_parent_markers_and_preserves_user_environment",
        ),
        passed,
    );
    // Ordinary no-APC traffic still has no image state or warm parser allocation.
    report(
        "M15.6 image-free parser zero allocation",
        m7::test(
            "puzzle-vt",
            "protocols::kitty::apc::tests::ordinary_terminal_feeds_allocate_nothing_after_warmup",
        ),
        passed,
    );
    // Includes the old twelve cases unchanged and four placeholder/seam crops.
    report(
        "M15.6 tile pixels at DPR 1/2 and existing image references",
        crate::golden::images::run(false, false),
        passed,
    );
    println!(
        "REVIEW M15.6 yazi placeholders in harness and app, plus new golden crops: docs/image-review.md"
    );
}

/// Real-client query ordering, unbounded-per-image puts and clipped holes are distinct seams.
fn yazi(passed: &mut bool) {
    // A reply that arrives after DA1 makes unknown-brand yazi choose another driver.
    // The shared-memory rejection must not delay or replace the direct-medium OK.
    report(
        "M15.6 exact yazi query replies before DA1",
        m7::test(
            "puzzle-embed",
            "session_thread::tests::images::yazi_exact_probe_precedes_da1_and_rejects_shared_memory",
        ),
        passed,
    );
    // Thousands of quiet source-cropped puts must not disappear behind a fixed cap.
    // Selective deletion also checks that one cell does not remove the shared source.
    report(
        "M15.6 yazi KgpOld 6000 crops and selective deletes",
        m7::test(
            "puzzle-term",
            "session::tests::images::yazi::yazi_kgp_old_many_quiet_crops_and_selective_deletion",
        ),
        passed,
    );
    // Dense batching may merge only printed cells, never fill an edited hole.
    report(
        "M15.6 placeholder holes share continuous image mapping",
        m7::test(
            "puzzle-term",
            "session::tests::images::placeholders::placeholder_clips_preserve_holes_and_share_the_full_transform",
        ),
        passed,
    );
}
