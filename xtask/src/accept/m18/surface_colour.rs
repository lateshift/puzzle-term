//! Swapchain acceptance observes the presented drawable rather than its oracle.
//! Separate host receipts prove winit, NSView and borrowed CAMetalLayer setup.
//! A host P3 layer is deliberately seeded before configuration: it must become
//! sRGB, with EDR disabled, so a wide-gamut monitor cannot stretch palette bytes.
//! Each child records only a first presentation and then synchronously releases
//! its surfaces. No global keyboard injection or personal shell command is used.
//! Raw JSON receipts survive a failed criterion for reviewer inspection.
//! Source-only format tests complement, but cannot replace, the native witness.
//! The gate checks exact eight-bit background bytes, with no golden tolerance.
//! Before the fix this reports [66,75,90] instead of theme [14,18,26].
//! Offscreen goldens remain independent: changing their expected values would
//! conceal the split between correct shaders and an incorrect swapchain format.
use super::report;
use std::{fs, process::Command};

/// Retains raw records even when a wrong attachment or byte conversion fails.
pub(super) fn run(passed: &mut bool) {
    report(
        "M18.surface-colour native drawable bytes and sRGB compositor",
        witness(),
        passed,
    );
    report(
        "M18.surface-colour explicit layer sRGB after configure and resize, both hosts",
        layer_witness(),
        passed,
    );
}

/// Exercises actual native surfaces with the ordinary frame executor and joined close.
fn witness() -> Result<(), String> {
    let directory = "target/m18/surface-colour";
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let c = format!("{directory}/native.jsonl");
    let harness = format!("{directory}/harness.jsonl");
    // Replace only this invocation's receipts; earlier before/after files stay separate.
    let host_log = format!("{c}.host.log");
    for path in [&c, &harness, &host_log] {
        if std::path::Path::new(path).exists() {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    // NSView and host CAMetalLayer run through the installed C header, not
    // private frame-builder entry points. The fixture also reads native metadata.
    crate::ffi::surface_colour(&c)?;
    // Build from current sources; stale target binaries cannot certify the repair.
    crate::commands::quiet("cargo", &["build", "-p", "puzzle-desktop"])?;
    // Winit uses the same App/Surface seam. It closes at FirstFrame without
    // asking a user shell to execute commands or consuming global keyboard focus.
    let output = Command::new("target/debug/puzzle-desktop")
        .args([
            "run",
            "--config",
            "/nonexistent-puzzle-colour-config.json",
            "--set",
            "theme.mode=dark",
            "--set",
            "theme.dark=puzzle-dark",
        ])
        .env("PUZZLE_SURFACE_COLOUR_REPORT", &harness)
        .env("PUZZLE_GPU_FAIL_ON_ERROR", "1")
        .env("SHELL", "/bin/sh")
        .output()
        .map_err(|e| e.to_string())?;
    // Preserve startup logs, including the once-per-surface format declaration.
    fs::write(format!("{directory}/harness.log"), &output.stderr).map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("native harness failed; see surface-colour/harness.log".into());
    }
    // Missing presentations fail too: an empty file is not evidence of correct ink.
    validate(&c, 2)?;
    validate(&harness, 1)
}

/// Every host must actually present, using non-sRGB bytes equal to the theme clear.
fn validate(path: &str, count: usize) -> Result<(), String> {
    // An absent receipt is a failing test, never an optional skip on this machine.
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    if text.lines().count() != count {
        return Err(format!(
            "{path}: expected {count} presented drawables: {text}"
        ));
    }
    // Every independently created target must obey both contracts. Equality of
    // actual/expected tests the complete queue path, not only capability selection.
    for line in text.lines() {
        let value: serde_json::Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if value["srgb_format"] != false
            || value["color_space"] != "Srgb"
            || value["layer_srgb"] != true
            || value["expected"] != serde_json::json!([14, 18, 26, 255])
            || value["actual"] != value["expected"]
            || value.get("error").is_some()
        {
            return Err(format!("{path}: encoded colour contract failed: {line}"));
        }
        // Emit decisive bytes in the acceptance output as well as the raw files.
        println!("M18_SURFACE_COLOUR {line}");
    }
    Ok(())
}

/// Native getters, independent of wgpu's enum, must prove all four lifecycle cases.
fn layer_witness() -> Result<(), String> {
    // host_output saves even assertion failures; an early C failure must not
    // discard the nil/P3 readback that explains why this contract failed.
    let path = "target/m18/surface-colour/native.jsonl.host.log";
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    // Track lifecycle cases rather than accepting four copies of one host.
    let mut cases = std::collections::BTreeSet::new();
    // C records include actual named native metadata after each asynchronous resize.
    for line in text
        .lines()
        .filter_map(|line| line.strip_prefix("M18_LAYER_COLOUR "))
    {
        // A malformed observation is a failed witness, never an ignored sample.
        let value: serde_json::Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        // Metal pixelFormat 80 is BGRA8Unorm. Metadata alone cannot detect
        // switching back to the sRGB attachment that double-encoded the bytes.
        if value["colorspace"] != "sRGB" || value["edr"] != 0 || value["pixel_format"] != 80 {
            return Err(format!(
                "native layer must be BGRA8Unorm with named sRGB and no EDR: {line}"
            ));
        }
        // Absent host/phase fields deliberately produce an unexpected case,
        // so an incomplete native record cannot satisfy the expected matrix.
        cases.insert((
            value["kind"].as_u64().unwrap_or(0),
            value["phase"].as_str().unwrap_or("").to_owned(),
        ));
    }
    // Native kind 1 asks wgpu to create an NSView sublayer; kind 5 borrows
    // the host layer itself. Both must survive wgpu's subsequent resize reset.
    let expected = [1, 5]
        .into_iter()
        .flat_map(|kind| {
            ["configure", "resize"]
                .into_iter()
                .map(move |phase| (kind, phase.to_owned()))
        })
        .collect();
    // Exact equality rejects omissions and unexpected lifecycle identifiers.
    // This check is independent of the first-presentation pixel witness above.
    if cases != expected {
        return Err(format!(
            "incomplete native configure/resize readbacks: {cases:?}"
        ));
    }
    Ok(())
}
