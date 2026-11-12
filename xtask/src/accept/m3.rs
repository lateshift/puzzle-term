//! M3 acceptance for the Slug GPU pipeline and review artifacts.
//!
//! The hinting comparison is a CPU-only side task within M3. Its success
//! cannot stand in for shader, texture, browser or oracle evidence. Each
//! independent obligation therefore gets its own producer and result line.
//!
//! The oracle has two layers after the §9.6 correction. Math fidelity
//! evaluates the CPU reference at the GPU's captured interpolated em
//! coordinates and derivatives. End-to-end comparison measures coordinate
//! displacement and attributes each pixel difference to the oracle's own
//! response at that displaced coordinate. Separate results distinguish a
//! porting error, a vertex interpolation error and an unexplained pixel.
//! The D-8 format comparison is another independent gate: it benchmarks
//! the same real-font glyph workload with f32 and f16 curve textures.
//! The web build is run from source on every acceptance attempt. A stale
//! `dist/web` artifact could otherwise make a broken browser shader or
//! missing bundled font appear ready for Chrome and Safari review.
use super::report;
use std::fs;

/// Renders all D-27 rows anew and checks the resulting review artifact.
fn hinting_comparison() -> Result<(), String> {
    // The renderer rejects a single blank specimen row. Running it here
    // prevents an old PNG from satisfying acceptance after a code change.
    crate::commands::quiet("cargo", &["xtask", "slug-dump", "--compare-hinting"])?;
    let sheet =
        fs::metadata("target/slug/hinting-compare.png").map_err(|error| error.to_string())?;
    if sheet.len() < 100 {
        return Err("D-27 comparison PNG is empty".into());
    }
    // Each variant has an independent nearest-neighbor crop so reviewers
    // can inspect its edge pixels without an image viewer's interpolation.
    for name in [
        "a", "b", "c", "d-b", "d-c", "e-b", "e-c", "f1-b", "f1-c", "f2-b", "f2-c",
    ] {
        let path = format!("target/slug/hinting-{name}-3x.png");
        let crop = fs::metadata(&path).map_err(|error| format!("{path}: {error}"))?;
        if crop.len() < 100 {
            return Err(format!("{path} is empty"));
        }
    }
    // A PNG header alone is much smaller than this sheet; the row-ink
    // assertions in the producer check content before it writes the file.
    Ok(())
}

/// Runs the native renderer's independent math or end-to-end oracle layer.
fn gpu_probe(mode: &str) -> Result<(), String> {
    // Separate process invocations prevent a failure in one oracle layer
    // from suppressing the other's acceptance result (R14).
    crate::commands::quiet(
        "cargo",
        &["run", "-p", "puzzle-desktop", "--", "slug-probe", mode],
    )
}

/// Measures D-8 on the real-font GPU batch and enforces its 10% decision gate.
fn precision_cost() -> Result<(), String> {
    // The benchmark performs a warmup and seven queue-completion samples
    // per format; its printed overhead is the measured decision value.
    let output = crate::commands::capture(
        "cargo",
        &["run", "-p", "puzzle-desktop", "--", "slug-bench"],
    )?;
    let value = output
        .split("overhead=")
        .nth(1)
        .and_then(|part| part.split('%').next())
        .ok_or("D-8 benchmark omitted its overhead percentage")?;
    // A missing or malformed value is a failed measurement, never a pass
    // justified by the process's exit code alone.
    let overhead: f64 = value
        .parse()
        .map_err(|_| "invalid D-8 overhead percentage")?;
    if !overhead.is_finite() || overhead > 10.0 {
        // D-8 requires user review above ten percent rather than silently
        // switching to lower precision or weakening the GPU budget (R5).
        return Err(format!(
            "Rgba32Float costs {overhead:.2}% over Rgba16Float; D-8 needs review"
        ));
    }
    Ok(())
}

/// Builds the browser scene and checks the shipped brotli size budget.
fn web_scene_build() -> Result<(), String> {
    // A native Metal render cannot establish that the same WGSL and font
    // asset link in the WebGPU-only wasm target used for browser review.
    let output = crate::commands::capture("cargo", &["xtask", "web", "build-review"])?;
    let value = output
        .split("brotli=")
        .nth(1)
        .and_then(|part| part.split_whitespace().next())
        .ok_or("web build omitted brotli size")?;
    let bytes: usize = value.parse().map_err(|_| "invalid brotli byte count")?;
    if bytes > 1_000_000 {
        return Err(format!("browser wasm is {bytes} B brotli, above 1 MB"));
    }
    Ok(())
}

/// Runs the browser shell's DPR/backing/CSS invariant on native test inputs.
///
/// The web crate keeps the sizing arithmetic in a target-independent
/// helper, so the acceptance gate can cover fractional display scaling
/// without treating a screenshot taken on this machine as browser proof.
fn canvas_sizing() -> Result<(), String> {
    // A filtered Cargo test can succeed with zero matches, so require the
    // named assertion in its output as well as a successful exit status.
    // The checked values include 1×, Windows 125% and 150%, and Retina 2×;
    // actual canvas presentation remains the user's Chrome/Safari review.
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-web", "canvas_size_invariant"],
    )?;
    if !output.contains("test canvas_size::tests::canvas_size_invariant ... ok") {
        return Err("browser canvas sizing test did not run".into());
    }
    Ok(())
}

/// Reports every automatable M3 criterion and the pending human reviews.
pub(super) fn run() -> bool {
    let mut passed = true;
    // Acceptance reruns the workspace gate to cover both target compiles,
    // the generated-file checks and all native unit/property tests.
    report(
        "native and wasm check",
        crate::commands::check(),
        &mut passed,
    );
    report(
        "D-27 comparison PNG and every variant row contains ink",
        hinting_comparison(),
        &mut passed,
    );
    report(
        "browser Slug scene builds under the 1 MB wasm target",
        web_scene_build(),
        &mut passed,
    );
    report(
        "browser backing equals round(CSS size × DPR)",
        canvas_sizing(),
        &mut passed,
    );
    // Three independent §9.6 results identify which stage fails.
    oracle_criteria(&mut passed);
    report(
        "D-8 f32 versus f16 GPU cost is at most 10%",
        precision_cost(),
        &mut passed,
    );
    // The oracle runs already exercise vertex expansion and first uploads.
    // Growth and nonzero paint need separate probes because neither case
    // occurs in the seven fresh-atlas oracle scenes.
    let store = crate::commands::quiet(
        "cargo",
        &["run", "-p", "puzzle-desktop", "--", "slug-store"],
    );
    let paint = crate::commands::quiet(
        "cargo",
        &["run", "-p", "puzzle-desktop", "--", "slug-paint"],
    );
    report(
        "instance expansion, texture growth/uploads and paint stub",
        store.and(paint),
        &mut passed,
    );
    // The scenario renders all seven named scenes anew, checks pixel and
    // ink assertions, then compares their bytes to reviewed references.
    report(
        "golden scenes at 6–96 px, transforms and DPR 1/2",
        crate::golden::run(false, false),
        &mut passed,
    );
    // R15 keeps image judgment and browser behavior in the review queue.
    println!("REVIEW golden references: assets/scenarios/slug/; README.md review steps");
    println!("REVIEW Chrome and Safari slug scene: web/?scene=slug; README.md review steps");
    println!(
        "REVIEW D-27 hinting comparison: target/slug/hinting-compare.png; target/slug/hinting-*-3x.png"
    );
    println!("REVIEW comment quality: crates/puzzle-render/src/; README.md review steps");
    passed
}

/// Reports shader math, affine placement and final coverage separately.
fn oracle_criteria(passed: &mut bool) {
    // All three commands render the same named matrix. Separate results
    // keep a vertex error visible even when coverage math itself passes.
    report(
        "Slug math matches CPU oracle at captured GPU inputs within 1/255",
        gpu_probe("math"),
        passed,
    );
    report(
        "GPU coordinates stay within 1/256 device pixel of exact affine",
        gpu_probe("coordinate"),
        passed,
    );
    report(
        "every GPU/CPU pixel deviation is explained within 1/255",
        gpu_probe("explained"),
        passed,
    );
}
