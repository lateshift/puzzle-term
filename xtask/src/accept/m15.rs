//! Ordered M15 desktop receipts; incomplete image tasks cannot claim acceptance.
//! Task selection exercises the APC seam without launching unrelated live work.
//! Each criterion reports independently; the final milestone will include the
//! storage, rendering, embedding and Chrome criteria as their tasks finish.
//!
//! Framing alone does not advertise image support: protocol replies and stored
//! placements arrive in M15.2. The graphics yield test deliberately checks the
//! parser boundary without pretending a fabricated reply came from a decoder.
//! Native corpus probes run after the functional tests, one at a time, so a
//! compiler or competing probe cannot become part of their timed workload.
//!
//! The budgets selector repeats only measurements during a regression diagnosis.
//! It cannot substitute for full acceptance: closure still runs every functional,
//! GPU and Chrome criterion. Reports retain the original floors and failed runs.
//! REVIEW output names the evidence without approving its visual appearance.
use super::{BrowserGate, bridge, m7, report};
/// C host settings, zero idle work and unchanged native image-free budgets.
mod embedding;
/// Unicode placeholder wire cells, sealed tiles and pixel references.
mod placeholders;
/// Native decoding and pure protocol/storage receipts stay separately attributable.
mod protocol;
/// Actual image pixels, visible residency and sealed anchor geometry.
mod rendering;
/// Deterministic no-image work protects scrolling and ordinary frame rebuilds.
mod text_work;
/// Reuse the image-free parser probes with their unchanged §15 floors.
mod throughput;

/// Run a named ordered task or the complete implemented milestone inventory.
pub(super) fn run(args: &[String]) -> bool {
    let mut passed = true;
    match args {
        // An explicit task receipt lets the work order advance one solid step.
        [flag, task] if flag == "--task" && task == "apc" => apc(&mut passed),
        [flag, task] if flag == "--task" && task == "protocol" => protocol::run(&mut passed),
        // The user reopened yazi support after rendering shipped; this subset
        // verifies that addition without rerunning load-sensitive numeric gates.
        [flag, task] if flag == "--task" && task == "placeholders" => {
            placeholders::run(&mut passed)
        }
        [flag, task] if flag == "--task" && task == "rendering" => rendering::run(&mut passed),
        [flag, task] if flag == "--task" && task == "embedding" => {
            throughput::run(&mut passed);
            embedding::run(&mut passed);
        }
        [flag, task] if flag == "--task" && task == "budgets" => {
            // Targeted reruns retain every established numeric line and skip only
            // unrelated functional criteria already recorded by the embedding task.
            throughput::run(&mut passed);
            embedding::budgets(&mut passed);
        }
        [flag, task] if flag == "--task" && task == "text-work" => text_work::run(&mut passed),
        [] => {
            text_work::run(&mut passed);
            apc(&mut passed);
            protocol::run(&mut passed);
            rendering::run(&mut passed);
            embedding::run(&mut passed);
            placeholders::run(&mut passed);
            // Native GPU work ends before browser ownership begins; no skip at closure.
            bridge::closure(BrowserGate::Full, &mut passed);
            report(
                "M15 full Chrome regression suite at DPR 1 and 2",
                crate::browser::suite().map(|_| ()),
                &mut passed,
            );
            // Mechanical checks cannot certify real-app appearance or comment quality.
            println!(
                "REVIEW M15 image references: assets/scenarios/images/; six cases at DPR 1/2, pending user approval"
            );
            println!(
                "REVIEW M15 yazi/chafa in macOS app and harness: PNG, converted JPEG, scrolling, clear/reset, resize; docs/image-review.md"
            );
            println!(
                "REVIEW M15 matched reference-terminal throughput: same display and geometry, user-run; docs/history/M15.md"
            );
            println!(
                "REVIEW M15 comment quality: protocol, admission, snapshot, renderer and native decoder; pending reviewer"
            );
        }
        // Unknown flags must not silently select an easier acceptance subset.
        _ => report(
            "M15 arguments",
            Err(
                "usage: accept M15 [--task apc|protocol|rendering|embedding|budgets|placeholders|text-work]"
                    .into(),
            ),
            &mut passed,
        ),
    }
    passed
}

/// Every input-boundary and allocation guarantee has an independently named line.
fn apc(passed: &mut bool) {
    for (criterion, test) in [
        // Even arbitrary malformed payload bytes must release a subsequent RIS.
        (
            "M15.1 arbitrary graphics bytes recover at RIS",
            "arbitrary_graphics_prefix_recovers_at_ris",
        ),
        (
            "M15.1 maximum chunk across 1-KiB/16-KiB boundaries",
            "maximum_chunk_crosses_kernel_reads_and_desktop_slices",
        ),
        // Transparency is byte-for-byte, including invalid UTF-8 and non-kitty APC.
        (
            "M15.1 random non-graphics bytes pass unchanged",
            "non_graphics_bytes_pass_unchanged",
        ),
        // Both bytes of ST and every point of the introducer can straddle reads.
        (
            "M15.1 every split, one-byte reads and other APC strings",
            "every_split_and_one_byte_reads_keep_graphics_and_other_apcs",
        ),
        // Overlong prefixes must not turn their surviving suffix into a command.
        (
            "M15.1 bounded control/payload and whole-string discard",
            "payload_and_control_caps_discard_whole_and_resume_after_st",
        ),
        // Cancellation/new escapes recover; ordinary unterminated text stays APC.
        (
            "M15.1 cancellation and malformed-string recovery",
            "cancellation_and_restarted_escape_do_not_swallow_following_output",
        ),
        // A semantic parser yield cannot advance prefix state past unparsed bytes.
        (
            "M15.1 semantic parser yields retain unconsumed suffix",
            "vt_partial_consumption_preserves_unaccepted_candidate",
        ),
        // Later DA and OSC responses stay behind the completed graphics request.
        (
            "M15.1 graphics, DA and OSC requests retain source order",
            "terminal_graphics_yield_precedes_da_and_registered_osc",
        ),
        // No graphics traffic means no payload reservation or new warm allocations.
        (
            "M15.1 no allocation on image-free warm parser path",
            "ordinary_terminal_feeds_allocate_nothing_after_warmup",
        ),
    ] {
        // Exact module-qualified names reject a renamed or missing test witness.
        // D-58 relocates the APC grammar to kitty; the public APC re-export
        // remains compatible, and these original assertions still run there.
        apc_case(criterion, test, passed);
    }
    // These rates keep the established native floors; new image costs are not
    // allowed to consume the budget of a terminal that displays only text.
    throughput::run(passed);
}

/// Runs an unchanged APC witness at its D-58 origin-qualified test path.
fn apc_case(criterion: &str, test: &str, passed: &mut bool) {
    // The exact test selector prevents a successful zero-test run after a move.
    report(
        criterion,
        m7::test(
            "puzzle-vt",
            &format!("protocols::kitty::apc::tests::{test}"),
        ),
        passed,
    );
}
