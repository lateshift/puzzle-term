//! OSC 66 acceptance keeps model, publication, rendering and unused work separate.
//!
//! VT conformance covers the wire grammar and atomic editing/reflow invariants.
//! Sealed publications must remain independent of later Session slot reuse.
//! Render tests check transformed ink, cursor extents and physical hit geometry.
//! Allocation and operation counters protect ordinary text when OSC 66 is unused.
//! The eight image candidates keep the existing numerical comparison tolerance;
//! human approval of their appearance remains a separate M18.9 review item.
//! Absolute performance lines stay in full acceptance, never inferred from counts.
use super::report;

/// Independent behavioral filters; each criterion runs even after an earlier failure.
const CHECKS: [(&str, &str, &str, &str); 8] = [
    // Wire tests include arbitrary operation interleavings. The consistency
    // checker validates every retained fragment after each mutation, including
    // resize and history eviction rather than only initial placement.
    (
        "M18.7 grammar, editing, reflow, contents and invariant properties",
        "puzzle-vt",
        "conformance",
        "text_sizing::",
    ),
    // Published text must survive live VT slot-key reuse. A root outside the
    // viewport still owns visible lower fragments, and forced warm recapture
    // must retain allocation-free storage reuse.
    (
        "M18.7 sealed payload lifetime, clipped roots and warm capture",
        "puzzle-term",
        "lib",
        "frame_snapshot::tests::multicell::",
    ),
    // Glyph tests check scaling through existing pipelines, not font settings.
    // Physical hit cells and cursor extents must agree with the transformed
    // ink, including pack-only first frames and rotating snapshot slots.
    (
        "M18.7 scaled glyphs, cursor extents, physical hits and cold equality",
        "puzzle-term",
        "lib",
        "frame_builder::tests::multicell::",
    ),
    // Overlay views read a frozen publication-local buffer. Their block payloads
    // cannot borrow mutable Session state, and retiring the last block must
    // restore the ordinary no-sparse-work path.
    (
        "M18.7 frozen overlay contents, retirement and warm allocations",
        "puzzle-overlay",
        "core",
        "text_sizing_",
    ),
    // Printing compares ASCII, single-cell Unicode and wide scalars at both
    // geometries. Positive controls prove that sparse lookup observers detect
    // real resolution rather than always returning zero.
    (
        "M18.7 ordinary scalar work and zero warm print allocations",
        "puzzle-vt",
        "lib",
        "terminal::tests::text_sizing_work::",
    ),
    // Plain and partial-region scrolls must move handles rather than cells.
    // Width-independent counts include empty/full history and the observer
    // has controls that deliberately clear or copy cells.
    (
        "M18.7 ordinary scrolling touches zero cells at both widths",
        "puzzle-vt",
        "lib",
        "ring::tests::operation_counts::",
    ),
    // Full ordinary builds retain the original copied/painted/shaped work.
    // Warm unchanged builds allocate nothing, and an empty retired feature
    // store must not introduce a separate frame scan.
    (
        "M18.7 ordinary frame work and zero warm assembly allocations",
        "puzzle-term",
        "lib",
        "text_sizing_ordinary_frames_keep_work_and_zero_allocations",
    ),
    // A live block must trip the frame observers. Removing it must restore
    // zero sparse work without relying on a fresh Session, protecting long
    // lived terminals that temporarily used the protocol.
    (
        "M18.7 sparse frame observer detects work and last-block retirement",
        "puzzle-term",
        "lib",
        "text_sizing_sparse_observer_and_last_block_retirement",
    ),
];

/// Reports independent behavior guarantees and the pending human reference review.
pub(super) fn run(passed: &mut bool) {
    for (criterion, package, target, filter) in CHECKS {
        // Each filter targets behavioral assertions, including positive counter controls.
        let mut args = vec!["test", "-p", package];
        if target == "lib" {
            args.push("--lib");
        } else {
            args.extend(["--test", target]);
        }
        args.push(filter);
        report(criterion, crate::commands::quiet("cargo", &args), passed);
    }
    // Regenerate before comparison; never certify a stale image or bless in acceptance.
    report(
        "M18.7 eight DPR/reflow references, ink, clipping and cold equality",
        crate::golden::text_sizing::run(false, false),
        passed,
    );
    println!(
        "REVIEW M18.7 headings/fractions/reflow and selection in both hosts: docs/protocol-review.md; eight assets/scenarios/text-sizing candidates; pending M18.9"
    );
}
