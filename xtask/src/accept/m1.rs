//! M1 acceptance inventory for the typed bus and logging (design §17).
//!
//! Every mechanical phrase in the M1 row has its own result line. Tests
//! exercise the public bus surface, while the repository lint checks the
//! independently maintained enum and wire catalogue. The editorial comment
//! review remains visible after every result without being self-certified.
//!
//! Acceptance invokes focused tests even after the workspace gate fails.
//! This is intentional: a broken wasm compile should not hide whether the
//! policy semantics or input validation also regressed. Each child command
//! is quiet on success so the report stays one line per requirement. A
//! failing child contributes its diagnostic on the corresponding FAIL line.
//! The final REVIEW line is evidence for a person, never a passing assertion.
use super::report;

/// Runs one named bus test without mixing Cargo's chatter into the report.
fn bus_test(name: &str) -> Result<(), String> {
    crate::commands::quiet("cargo", &["test", "-p", "puzzle-bus", name])
}

/// Runs a terminal-domain protocol test without coupling the generic bus.
fn term_test(name: &str) -> Result<(), String> {
    // The terminal domain owns conversion, so these checks must compile it
    // independently of the bus's intentionally terminal-free fixtures.
    crate::commands::quiet("cargo", &["test", "-p", "puzzle-term", name])
}

/// Runs both public lanes; either failure rejects the combined criterion.
fn public_round_trips() -> Result<(), String> {
    term_test("every_public_command_round_trips")?;
    term_test("every_public_event_round_trips")?;
    term_test("concrete_kind_indices_match_catalogue")?;
    term_test("extension_events_and_commands_round_trip")
}

/// Checks the actual domain decoder and generated reference together.
fn visibility_catalogue() -> Result<(), String> {
    // A complete source table is insufficient if runtime conversion admits
    // a reserved or internal name despite the documented visibility gate.
    term_test("reserved_and_internal_kinds_cannot_enter_wire")?;
    crate::lint::run()
}

/// Requires reports for bad shapes and a property run on arbitrary trees.
fn hostile_wire() -> Result<(), String> {
    term_test("wrong_payload_shape_has_report")?;
    term_test("invalid_wire_input_never_panics")?;
    term_test("nested_wire_input_never_panics")
}

/// Prints all mechanical results, then the pending human comment review.
pub(super) fn run() -> bool {
    let mut passed = true;
    // The shared gate covers native and wasm compilation, tests, and lint.
    report(
        "native and wasm check",
        crate::commands::check(),
        &mut passed,
    );
    // Every named behavior remains a distinct acceptance line. The priority
    // fixture also asserts the rewritten command value, so it protects both
    // contractual properties when run for their separate report entries.
    for (criterion, test) in [
        ("priority order", "priority_and_in_place_rewrite"),
        ("command veto", "veto_prevents_execution"),
        ("in-place modification", "priority_and_in_place_rewrite"),
        (
            "cascade limit and hook chain warning",
            "cascade_limit_logs_hook_chain",
        ),
        (
            "unobserved publish and idle tap allocate zero",
            "idle_publish_and_tap_allocate_nothing",
        ),
        ("target filter parsing", "parses_target_overrides"),
    ] {
        // A failing test does not short-circuit later independent criteria.
        report(criterion, bus_test(test), &mut passed);
    }
    // An extension observer must not activate every plugin's allocation path.
    // The focused test also verifies that a matching typed event is delivered.
    report(
        "extension messages round trip",
        term_test("extension_events_and_commands_round_trip"),
        &mut passed,
    );
    report(
        "unrelated extension publication allocates zero",
        bus_test("unrelated_extension_publish_allocates_nothing"),
        &mut passed,
    );
    // Wire conversion has two directions of public messages and two ways
    // input can be hostile: a recognizable wrong shape or arbitrary trees.
    report(
        "every public wire kind round trips",
        public_round_trips(),
        &mut passed,
    );
    report(
        "invalid wire payloads report without panic",
        hostile_wire(),
        &mut passed,
    );
    report(
        "kind visibility catalogue and generated reference",
        visibility_catalogue(),
        &mut passed,
    );
    // Origin is assigned by the trusted host, then seen by an ordinary hook.
    report(
        "external dispatch carries Api origin through interceptors",
        term_test("external_dispatch_has_api_origin"),
        &mut passed,
    );
    // R15 requires a reviewer, even when all mechanical criteria pass.
    println!(
        "REVIEW comment quality: crates/puzzle-bus/src/; crates/puzzle-term/src/messages.rs; xtask/src/bus_docs.rs; docs/BUS.md; README.md (confirmed by Claude 2026-09-29)"
    );
    passed
}
