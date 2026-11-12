//! M15.4 exercises settings through the real C host and retains image-free budgets.
//! No host-specific image callback exists: ordinary output carries raw/PNG data.
//! Native timings use the established optimized probes with unchanged thresholds.
//! Each live run is archived before another run can overwrite its legacy paths.
//!
//! The C fixture changes both application defaults and a surface override. It
//! feeds graphics through the public byte transport and follows each request
//! with DA, proving asynchronous decode preserves the owner's reply order.
//! Disabling storage must remove existing sources; re-enabling cannot revive
//! them. This covers live settings rather than only schema validation.
//!
//! Functional idle checks use an injected clock and a counting allocator.
//! Speed checks instead run optimized code, after compilation has completed.
//! Full-screen ASCII and one-row medians share one probe; Unicode uses the
//! installed vtebench source with its actual byte count and producer table.
//! GPU readbacks perturb cadence, so ink and ordinary frame timing need two
//! independent windows. The latter retains full attempted-frame chronology.
//!
//! Every threshold is inherited from §15, including the derived 120-Hz p95
//! allowance. Adding images cannot excuse slower text-only workloads, and an
//! empty or incomplete report is a failure rather than a zero-cost sample.
use crate::accept::{m6, m7, m16, report};

/// Functional embedding precedes serial release measurements on the same machine.
pub(super) fn run(passed: &mut bool) {
    // The linked caller tests app inheritance, surface overrides and ordered replies.
    report(
        "M15.4 C host image settings and raw/PNG byte transport",
        crate::ffi::host(),
        passed,
    );
    // Both image-free and image-bearing frames must reuse their allocations.
    report(
        "M15.4 warm image assembly and idle: zero allocations/frames",
        m7::test(
            "puzzle-term",
            "session::tests::image_idle::image_screen_warm_assembly_and_idle_allocate_nothing",
        ),
        passed,
    );
    budgets(passed);
}

/// Runs only the image-free M15.4 budget lines after an explicitly requested repair.
pub(super) fn budgets(passed: &mut bool) {
    // One optimized probe supplies both medians, keeping the geometry comparable.
    let measured = m6::frame::cpu_budget();
    if let Ok((full, row)) = &measured {
        println!("M15_METRIC image-free full_us={full:.6} row_us={row:.6}");
    }
    report(
        "M15.4 image-free full 240x70 <=2 ms",
        measured
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|(full, _)| m6::frame::within(*full, 2000.0, "full frame")),
        passed,
    );
    report(
        "M15.4 image-free changed row <=0.2 ms",
        measured
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|(_, row)| m6::frame::within(*row, 200.0, "one row")),
        passed,
    );
    // Compilation ends before either window starts its producing interval.
    let built = m16::complete::release();
    report(
        "M15.4 release measurement executable",
        built.clone(),
        passed,
    );
    if built.is_ok() {
        // The existing probes select the actual 60-Hz LG, not the built-in screen.
        // GPU ink reads and ordinary cadence remain independent observations.
        report(
            "M15.4 maximized image-free Unicode PTY >=130 MiB/s",
            live(false),
            passed,
        );
        report(
            "M15.4 warm Unicode frame median and 120-Hz allowance",
            live(true),
            passed,
        );
    }
}

/// Preserve old and fresh raw reports even if the new run fails its threshold.
///
/// The existing probe removes its live report before launch. Archiving beforehand
/// retains the previous observation, while the current subdirectory identifies
/// this invocation. Assessment happens only after the window exits, so copying
/// files or sorting percentiles cannot consume time during the measured interval.
fn live(cadence: bool) -> Result<(), String> {
    let label = if cadence {
        "m165-cadence"
    } else {
        "bitmap-maximized"
    };
    // A unique directory makes every attempted measurement attributable after reruns.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let directory = format!("target/m15-4/{stamp}-{label}");
    retain(label, &format!("{directory}/before"))?;
    // Load and assess only a fresh report produced by this exact process invocation.
    let result = if cadence {
        m16::complete::maximized_cadence()
            .and_then(|value| m16::throughput::budgets::frames::assess(&value))
    } else {
        m16::complete::maximized_unicode().and_then(|_| m16::throughput::budgets::live())
    };
    // Archive failures too: never retain only the passing cohort or a chosen sample.
    let saved = retain(label, &format!("{directory}/current"));
    println!("M15_EVIDENCE {directory}");
    result.and(saved)
}

/// Legacy probes share artifact names; copy their complete set between executions.
/// JSON carries stage clocks, PNG supplies visual evidence, and the producer's
/// table carries all transfer samples. Both process logs explain startup or
/// shutdown failures that might otherwise look like missing timing data.
fn retain(label: &str, directory: &str) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    for suffix in [".json", ".png", ".log", "-window.log", "-vtebench.dat"] {
        let name = format!("{label}{suffix}");
        let source = std::path::Path::new("target/m16-2").join(&name);
        // Before a first run no old file exists; a failed new run stays visibly absent.
        if source.is_file() {
            std::fs::copy(source, std::path::Path::new(directory).join(name))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
