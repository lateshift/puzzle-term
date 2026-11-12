//! M15.2 protocol/state and native worker checks exercise public byte boundaries.
//! Each independently reported result retains a failure even when another action
//! succeeds. Pure tests inject only decoded values; native tests use real codecs.
//! No file medium is tested as a success, because receiving bytes over SSH must
//! never grant a remote application permission to read the terminal's filesystem.
//! Ordinary bytes and keys remain different lanes while a worker owns pixels:
//! application replies preserve FIFO, while user input never waits for decoding.
//! Screen-domain anchors prove that a full-screen application's images cannot
//! erase a shell's retained images just because both rings use the same row IDs.
//! The zero-allocation check opts in to native images but sends only text, so it
//! detects costs that a parser-only, graphics-disabled control could conceal.
use crate::accept::{m7, report};

/// Every acceptance line names its exact test; a missing witness is a failure.
pub(super) fn run(passed: &mut bool) {
    for (criterion, package, test) in CASES {
        // Each witness has its own receipt; missing test names fail in m7::test.
        report(criterion, m7::test(package, test), passed);
    }
}

/// Exact byte-boundary witnesses, shared by task and final milestone acceptance.
/// Separate invocations retain every failure instead of aborting at the first one.
/// These fixtures need no renderer: image storage belongs to the Session owner.
/// Native codec fixtures are kept out of the shared-crate/wasm dependency graph.
/// The full milestone adds drawing, residency and live review after this checkpoint.
/// No protocol-only success certifies image pixels or GPU lifecycle behaviour.
const CASES: &[(&str, &str, &str)] = &[
    // Final-chunk admission, quiet policy and failed replacement preserve old content.
    (
        "M15.2 actions/chunks/quiet/atomic replacement",
        "puzzle-term",
        "images::tests::actions_chunking_quiet_and_atomic_failure",
    ),
    // Default display advances by the entire rectangle; C=1 leaves both axes alone.
    // Exercise both combined transmission and putting an already decoded source.
    (
        "M15.2 placement cursor policy and both axes",
        "puzzle-term",
        "images::tests::placement_cursor_policy_moves_both_axes_or_preserves_cursor",
    ),
    // All supported deletion letters have distinct retained-placement witnesses.
    (
        "M15.2 every deletion selector",
        "puzzle-term",
        "images::tests::every_delete_selector_preserves_unselected_placements",
    ),
    // Anchors follow row identity and belong to one screen, including erased history.
    (
        "M15.2 reflow/screens/clear/history/RIS",
        "puzzle-term",
        "images::tests::placement_screen_reflow_clear_and_history_lifetime",
    ),
    // Quota admission evicts hidden images before visible ones; zero disables storage.
    (
        "M15.2 decoded allowance/nonvisible LRU/disable",
        "puzzle-term",
        "images::tests::decoded_budget_eviction_and_zero_disable",
    ),
    // Unsupported media never touch the filesystem or leave an accumulator behind.
    (
        "M15.2 unsupported features and continuation quiet errors",
        "puzzle-term",
        "images::tests::unsupported_features_and_continuation_errors_reply_once",
    ),
    // Placement metadata cannot grow without bound through tiny repeated puts.
    (
        "M15.2 placement metadata admission/replacement",
        "puzzle-term",
        "images::tests::placement_metadata_replacement_stays_atomic_at_capacity",
    ),
    // Hostile payloads cannot panic or commit a prefix as a valid image.
    (
        "M15.2 malformed/oversized payload proptest",
        "puzzle-term",
        "images::tests::malformed_transmissions_leave_no_partial_state",
    ),
    // Control syntax remains bounded independently of decoded image dimensions.
    (
        "M15.2 arbitrary control proptest",
        "puzzle-term",
        "protocols::kitty::graphics::control::tests::arbitrary_controls_are_bounded",
    ),
    // A malformed later quartet must not append its earlier valid prefix.
    (
        "M15.2 Base64 atomic validation",
        "puzzle-term",
        "protocols::kitty::graphics::base64::tests::invalid_chunk_does_not_mutate_transfer",
    ),
    // Keys bypass native decoding, while application replies remain FIFO.
    (
        "M15.2 decode preserves key priority and reply order",
        "puzzle-term",
        "session::images::tests::decode_pauses_output_but_input_stays_immediate_and_replies_ordered",
    ),
    // Opted-in image support must preserve the warm no-allocation text path.
    (
        "M15.2 enabled image-free warm parse allocates nothing",
        "puzzle-term",
        "session::images::tests::enabled_images_without_graphics_allocate_nothing_when_warm",
    ),
    // Raw channels and zlib lengths validate before immutable pixels are published.
    (
        "M15.2 native raw/zlib exact pixels and bounds",
        "puzzle-embed",
        "image_decode::tests::raw_and_zlib_decode_exact_pixels_and_reject_bombs",
    ),
    // PNG alpha, transformed channels and compressed byte limits use real decoding.
    (
        "M15.2 native PNG transparency/zlib/bounds",
        "puzzle-embed",
        "image_decode::tests::png_pixels_and_compression_obey_decoded_allowance",
    ),
    // Several owners share a bounded worker and preserve buffered output at EOF.
    (
        "M15.2 bounded shared decoder and joined EOF",
        "puzzle-embed",
        "session_thread::tests::images::shared_image_worker_preserves_replies_and_joins_at_eof",
    ),
    // Overlapping numeric row IDs in two rings must never alias their tracked anchors.
    (
        "M15.2 tracked anchors have separate screen domains",
        "puzzle-vt",
        "tracked::tests::terminal_resize_and_alternate_clear_keep_primary_anchor",
    ),
];
