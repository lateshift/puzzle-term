//! Host byte sources retain the fixed-pool guarantees of native PTYs.
//! These witnesses separate exact admission from Session lifecycle and GPU work.
//! A byte-perfect pool test cannot establish EOF ordering or callback affinity;
//! a real Session test cannot observe every arbitrary byte after VT interpretation.
//! Together they prove both sides of that boundary without an unbounded recorder.
//! The release flood submits complete frames through the production GPU executor.
//! It measures host transport isolation, not a compositor's physical refresh rate.
//! Debug keeps the same functional workflow with a generous deadlock guard.
use crate::accept::report;

/// Runs each independent source contract, retaining failures instead of short-circuiting.
pub(super) fn run(passed: &mut bool) {
    for name in [
        "pty_pool::tests::admission::wake_tokens_cannot_lose_admission_storage_or_bytes",
        "pty_pool::tests::admission::host_hundred_mb_exact_and_bounded",
        "pty_pool::tests::admission::warm_admission_allocates_nothing",
        "app::tests::transport::public_host_hundred_mb_exact_and_bounded",
        "app::tests::transport::host_echo_order_resize_exit_and_affinity",
        "app::tests::transport::pty_surface_rejects_host_output",
    ] {
        // An exact-name check rejects Cargo's otherwise successful empty filter.
        report(
            &format!("M17.transport {name}"),
            crate::accept::m7::test("puzzle-embed", name),
            passed,
        );
    }
    // Keep optimized speed limits out of parallel-debug functional checks.
    report(
        "M17.transport real GPU flood >=30 fps and host input <=50 ms",
        super::release_test("app::tests::transport_flood::host_flood_and_key_budgets"),
        passed,
    );
    // The witness writes its exact measured interval after GPU warm-up.
    if let Ok(receipt) = std::fs::read_to_string("target/m17-transport-flood.txt") {
        println!("MEASURE {receipt}");
    }
}
