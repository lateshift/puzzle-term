//! Core-to-browser ownership contracts used by the M7.6 runtime.
//!
//! These tests need no browser or GPU. They pin the boundary where the
//! synchronous Send core releases typed work before a host calls JS, and
//! where ordinary key policies decide whether native textarea input owns
//! a press. Keeping them independent catches defects before DOM callbacks
//! and transport integration can conceal an ordering mistake.
use super::{report, test};

/// Reports each handoff separately so a veto defect cannot hide a FIFO defect.
pub(super) fn run(passed: &mut bool) {
    // All native flags run, but only Tint rejects the original raster uniformity bug.
    // Discovering modules still catches parse/type defects in shaders unused by a scene.
    // wgpu's native re-export keeps this compiler out of the WebGPU-only wasm package.
    report(
        "all WGSL modules pass native Naga parsing/general validation with all flags",
        test(
            "puzzle-desktop",
            "shader_validation::tests::every_shader_validates_with_uniformity",
        ),
        passed,
    );
    // First-frame geometry must be independent of the later TTF fetch.
    // The witness constructs a pack-only TextContext rather than a preloaded FontSet.
    // Takeover must retain both the cell advance and em-space atlas addresses.
    // Low-DPR hinted keys arrive separately; changing contours must not shift the grid.
    report(
        "pack-first frames preserve geometry and reuse unhinted takeover",
        test(
            "puzzle-term",
            "frame_builder::context::tests::pack_first_frame_and_font_takeover",
        ),
        passed,
    );
    // Reject a mismatched source without invalidating the already usable pack.
    report(
        "pack takeover rejects unrelated font bytes transactionally",
        test(
            "puzzle-term",
            "frame_builder::context::tests::takeover_rejects_wrong_font_transactionally",
        ),
        passed,
    );
    // Observer masks remain the source of truth: buffering is a delivery
    // mechanism, not an unfiltered callback lane alongside the typed bus.
    report(
        "buffered host events preserve observer masks and disposal",
        test(
            "puzzle-bus",
            "bus::tests::outbox::buffered_observers_keep_filters_and_disposal",
        ),
        passed,
    );
    // Text commits and parser replies must leave the core in dispatch order,
    // with no frame or foreign callback required to flush their bytes.
    report(
        "deferred input/reply FIFO and coalesced wake requests",
        test(
            "puzzle-term",
            "session::tests::host::deferred_effects_preserve_input_reply_fifo",
        ),
        passed,
    );
    // Identical consecutive leaders must flush as individual presses;
    // allowing native text too would duplicate the encoder's held keys.
    report(
        "native text ownership preserves shortcuts and repeated leader FIFO",
        test(
            "puzzle-term",
            "session::tests::host::native_text_bindings_and_repeated_leader_fifo",
        ),
        passed,
    );
    // Native DOM insertion cannot bypass a higher-priority host veto, and
    // a held process-exit screen consumes its next key to request closure.
    report(
        "native text ownership respects veto and held process exit",
        test(
            "puzzle-term",
            "session::tests::host::native_text_respects_veto_and_held_exit",
        ),
        passed,
    );
}
