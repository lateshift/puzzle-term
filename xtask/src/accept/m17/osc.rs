//! Optional OSC acceptance uses exact protocol witnesses and a default dependency tree.
//! Linking an optional implementation and registering it are separate host choices.
//! No protocol-specific variant or default terminfo capability is required.
//!
//! The protocol tests exercise the adopted revision 0.3 (2026-10-07), including
//! malformed-pair skipping and absence-as-indeterminate progress. Exact-name
//! witnesses fail if a later refactor removes or renames a required contract.
//! Session tests separately prove parser ordering, lifecycle and sync boundaries;
//! a parser-independent handler test cannot establish those integration rules.
//! The embedding witness enables the optional Cargo feature and registers it at
//! runtime through Surface, then receives a typed delta during App::tick.
//! The ordinary dependency tree is checked independently: a passing enabled build
//! does not prove the protocol remains absent from the default host binary.
use crate::accept::report;

/// Runs ordered parser, draft-validation and lifecycle contracts independently.
pub(super) fn run(passed: &mut bool) {
    for name in [
        // Earlier and later core queries must not overtake a registered handler.
        "registered_osc_replies_follow_source_order_across_chunks",
        // Prompt, RIS and exit handling must retain source order across callback yields.
        "generic_lifecycle_and_reset_are_source_ordered",
        // Without registration the parser consumes unknown OSCs without a reply.
        "unknown_osc_is_inert_and_builtins_cannot_be_shadowed",
    ] {
        report(
            &format!("M17.osc {name}"),
            crate::accept::m7::test("puzzle-term", &format!("session::tests::osc::{name}")),
            passed,
        );
    }
    run_extra(passed);
}

// Distribution-specific witnesses remain beside their optional implementation.
include!("osc/optional_items.rs");
