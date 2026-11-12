//! M12 task gates grow in the user's daily-driver order; full closure is later.
//! Layer ownership, bounded geometry admission and actual native contents get
//! separate criteria. No timing baseline or transferred refresh row runs here.
//!
//! Native contents are a different presentation destination, not another glyph
//! renderer. Exact real-surface byte checks catch format conversions that would
//! be invisible in existing offscreen references. The ICC profile travels with
//! IOSurface contents, so checking the unused container's colorspace is insufficient.
//! The C fixture reads both the native image and its metadata after real resize.
//!
//! Functional clock-injected tests protect unchanged-geometry nonblocking calls,
//! bounded waits, timeout preservation and close wakeups. A test filter is accepted
//! only if each named regression actually ran. Missing native hardware is a failure,
//! not a successful empty result. Human resize quality stays a review item.
//!
//! Host coverage checks geometry/callback contracts without a display timing round.
use super::report;
/// Default sRGB facts let embedders match replaceable native chrome.
mod colors;
/// Runtime input clocks preserve ABI and keep hardware observations one-shot.
mod input_latency;
/// Native UI coverage reserves chrome placement without shrinking the grid.
mod occlusion;
/// Native find components use the product search/selection contract (D-61).
mod search;
/// Exact fractional split geometry cannot suppress native first presentation.
mod split_presentation;
/// Native tab UI consumes shared command/activity contracts, never OSC.
mod tabs;

/// Run only the selected daily-driver task, never silently accept an unfinished milestone.
pub(super) fn run(args: &[String]) -> bool {
    // Full M12 acceptance grows with the remaining daily-driver tasks. An
    // unsupported invocation must never silently certify the whole milestone.
    // Shell label parsing and real snippet sources can be verified while the
    // independent input-latency observation still waits for a quiet machine.
    if args == ["--task", "tabs-snapshot"] {
        // Snapshot has its own optimized copy budget; it does not run the held
        // hardware key-resize cohort or certify unrelated unfinished M12 work.
        return tabs::snapshot();
    }
    if args == ["--task", "tabs-activity"] {
        return tabs::activity();
    }
    if args == ["--task", "tabs-command"] {
        return tabs::command();
    }
    if args == ["--task", "split-presentation"] {
        return split_presentation::run();
    }
    if args == ["--task", "input-latency"] {
        return input_latency::run();
    }
    if args == ["--task", "search"] {
        return search::run();
    }
    if args == ["--task", "colors"] {
        // Colour delivery is independent of the layer task's real-surface readback.
        return colors::run();
    }
    if args == ["--task", "occlusion"] {
        return occlusion::run();
    }
    if args != ["--task", "layer"] {
        println!(
            "FAIL M12 arguments: currently supported: --task layer | --task search | --task colors | --task occlusion | --task input-latency | --task split-presentation | --task tabs-command | --task tabs-activity | --task tabs-snapshot"
        );
        return false;
    }
    // Independent results continue after failures so the receipt is complete.
    let mut passed = true;
    report(
        "M12.layer geometry waits, timeout fallback, queue order and joined lifetime",
        native_tests(),
        &mut passed,
    );
    // Native image ownership cannot prove Session message ordering. Keep a
    // separate owner-side criterion: intervening keys must see their preceding
    // geometry even while later drag sizes replace one another before reflow.
    report(
        "M12.layer owner-turn geometry coalescing preserves input barriers",
        owner_geometry(),
        &mut passed,
    );
    report(
        "M12.layer checked C header and native ABI declarations",
        crate::ffi::verify(),
        &mut passed,
    );
    report(
        "M12.layer real NSView/host-layer IOSurface contents, bytes, sRGB and resize",
        native_contents(),
        &mut passed,
    );
    report(
        "M12.layer C host input, independent refresh takeover and joined close under output",
        crate::ffi::host(),
        &mut passed,
    );
    println!(
        "REVIEW M12.layer: edge drags, Magnet jumps and display moves in harness/app; docs/native-presentation.md"
    );
    passed
}
/// Actual native targets keep strict exact-byte and profile checks from M18.
fn native_contents() -> Result<(), String> {
    let directory = "target/m12-layer";
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    // Replace only this invocation's colour receipt; earlier runs have their
    // own logs and remain available for review, including timeout samples.
    let path = format!("{directory}/native-colour.jsonl");
    if std::path::Path::new(&path).exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    // Kind 1 uses an NSView, kind 5 a borrowed host CAMetalLayer. Both execute
    // abrupt size/scale changes without an AppKit live-resize notification.
    crate::ffi::surface_colour(&path)?;
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if text.lines().count() != 2 {
        return Err("both native targets must submit real contents".into());
    }
    // An error or missing image cannot be mistaken for equal black pixels.
    // Theme bytes remain exact; this is not a visual tolerance adjustment.
    for line in text.lines() {
        let record: serde_json::Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if record["presentation"] != "IOSurface"
            || record["layer_srgb"] != true
            || record["srgb_format"] != false
            || record["actual"] != record["expected"]
            || record["expected"] != serde_json::json!([14, 18, 26, 255])
            || record.get("error").is_some()
        {
            return Err(format!("native contents contract failed: {line}"));
        }
    }
    Ok(())
}

/// An accidentally renamed/empty filter cannot certify the native lifetime rules.
fn native_tests() -> Result<(), String> {
    let output = crate::commands::capture("cargo", &["test", "-p", "puzzle-sys", "presentation"])?;
    // Match each regression result rather than trusting a zero-exit test filter.
    // Native compilation alone cannot establish callback teardown or reuse rules.
    // The raw Cargo output remains part of a failure, including test diagnostics.
    for name in [
        "unchanged_geometry_never_waits_and_timeout_retains_previous_frame",
        "close_wakes_waiting_geometry_callback_without_waiting_for_gpu",
        "imported_images_complete_in_order_preserve_colour_and_bound_pool_ownership",
        "retained_completion_cannot_call_host_after_close",
        "geometry_wait_credit_requires_an_answer_and_never_repeats_a_size",
        "allocation_growth_and_contents_crop_preserve_exact_pixels",
        "compositor_owned_pool_never_admits_a_geometry_wait",
        "transferred_pending_geometry_cannot_pin_the_last_async_target",
        "transferred_or_declined_waits_never_gate_latest_async_demand",
    ] {
        if !output.contains(&format!("::{name} ... ok")) {
            return Err(format!("missing test result: {name}"));
        }
    }
    Ok(())
}

/// Reflow and ioctl coalescing is functional, without a debug timing assertion.
/// A burst must retain the final grid, surface extent and PTY size separately;
/// using elapsed time here would confuse correctness with debug scheduling.
fn owner_geometry() -> Result<(), String> {
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-embed", "session_thread::geometry"],
    )?;
    // Cargo can succeed when a filter matches nothing. Require the named result
    // so a renamed or removed ordering regression cannot silently certify this seam.
    if !output.contains("::intermediate_sizes_coalesce_but_input_barriers_keep_order ... ok") {
        return Err(format!("missing owner geometry regression: {output}"));
    }
    Ok(())
}
