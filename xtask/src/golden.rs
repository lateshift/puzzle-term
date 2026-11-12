//! Reviewed M3 GPU image references and the §2.7 numerical tolerance.
//!
//! A fresh native harness run supplies the complete M3 GPU matrix. Each script
//! screenshot also has `expect-pixel` and `expect-ink` assertions, so a blank
//! render cannot become a blessed reference accidentally. Normal runs read
//! the committed images and count pixels with any channel beyond ±2 bytes;
//! only 0.1% of image pixels may exceed that bound. The separate CPU oracle
//! checks catch Slug math errors even when a reference image is updated.
//!
//! `--bless` copies the newly rendered files into assets as pending review.
//! It never certifies their appearance (R15); the reviewer inspects those
//! exact files before confirming the M3 reference item in README.md.
//!
//! References crop the text rather than a full terminal window. This
//! keeps a changed pixel local to a size or transform. The scene order
//! is pinned here because the harness numbers screenshots by script position;
//! adding an assertion must not silently rename an existing reference.
//! The PNG reader rejects unsupported compression and filters, so a future
//! switch of writer format will fail clearly instead of comparing garbage.
//!
//! M4 adds an independent text scenario after shaping and the glyph seam.
//! Its fourteen references document regular, bold, italic, ligature choices,
//! Nerd Font fallback, tofu and 1×/2× cell joins. They share the same
//! image comparator, but M3 acceptance remains scoped to its earlier
//! 33-scene geometry matrix. This keeps old milestone gates meaningful.
//!
//! M8 adds fractional viewport crops at both hinting paths. The extra edge
//! row belongs to assembly, but the target and scissor retain the fixed grid
//! dimensions. The fixture checks padding independently of image comparison,
//! and requires glyph ink in both clipped strips rather than accepting their
//! colored cell backgrounds as evidence of text. Thus a scissor regression
//! or a missing edge row cannot be hidden by recording a new candidate PNG.
//! These two references remain pending human review until the M8 review STOP.
/// COLR gradients, consecutive layers and transformed clipping at both DPRs.
pub(crate) mod color;
/// Kitty raster placements and viewport/reflow crops at both device ratios.
pub(crate) mod images;
/// OSC selection colours use the shared table in actual GPU frames.
pub(crate) mod osc_colors;
/// Public provider shapes, bundled free text and grid effects at both DPRs.
pub(crate) mod overlays;
/// Literal match/current fills use their own independent GPU cell assertions.
pub(crate) mod search;
/// Selection image matrix shares the fixed comparator without changing old gates.
pub(crate) mod selection;
/// OSC 66 headings, fractional maps and atomic reflow at both device ratios.
pub(crate) mod text_sizing;
use crate::png;
use std::{fs, path::Path};

/// Runs the GPU scenario and compares or updates every M3 reference.
///
/// Acceptance suppresses per-scene success lines so R14 retains one result
/// for the golden criterion; the interactive command reports each artifact.
pub(crate) fn run(bless: bool, verbose: bool) -> Result<(), String> {
    // Rerender before comparing, so stale target files cannot make a new
    // shader or upload change appear to pass the golden suite.
    crate::commands::quiet(
        "cargo",
        &[
            "run",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/slug.pzs",
        ],
    )?;
    for (index, name) in expected_names().iter().enumerate() {
        let file = format!("{:03}-{name}.png", index + 1);
        let actual = Path::new("target/harness/slug").join(&file);
        let expected = Path::new("assets/scenarios/slug").join(&file);
        if bless {
            // Only the expected scene files are copied; logs and
            // unreviewed failure artifacts stay in target/.
            fs::create_dir_all("assets/scenarios/slug").map_err(|error| error.to_string())?;
            fs::copy(&actual, &expected).map_err(|error| error.to_string())?;
            if verbose {
                println!("BLESSED {}", expected.display());
            }
        } else {
            compare(&actual, &expected)?;
            if verbose {
                println!("PASS golden {name}");
            }
        }
    }
    Ok(())
}

/// Runs both reviewed scene families when a developer invokes `golden`.
pub(crate) fn run_all(bless: bool, verbose: bool) -> Result<(), String> {
    // M3 acceptance calls `run` for its original Slug matrix. The global
    // developer command also checks M4 text without making the older
    // milestone depend on a newer reference set.
    run(bless, verbose)?;
    run_text(bless, verbose)?;
    run_terminal(bless, verbose)?;
    run_scroll(bless, verbose)?;
    selection::run(bless, verbose)?;
    search::run(bless, verbose)?;
    color::run(bless, verbose)?;
    overlays::run(bless, verbose)?;
    images::run(bless, verbose)?;
    osc_colors::run(bless, verbose)?;
    text_sizing::run(bless, verbose)
}

/// Compares the retained-terminal scene crops, including the four cursor cases.
pub(crate) fn run_terminal(bless: bool, verbose: bool) -> Result<(), String> {
    // Fresh VT bytes, row caches and GPU execution run before comparison;
    // an old target PNG cannot make a changed cursor or SGR path pass.
    // This script also checks its own text prefixes, red background pixel
    // and ink occupancy before any candidate reference is considered.
    crate::commands::quiet(
        "cargo",
        &[
            "run",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/terminal-frame.pzs",
        ],
    )?;
    for (index, name) in [
        "colors-decorations",
        "block-ascii-cursor",
        "bar-cursor",
        "underline-cursor",
        "block-wide-cursor",
        "wide-box",
    ]
    .iter()
    .enumerate()
    {
        // A grid dump uses one scenario sequence number but is not a PNG.
        // The explicit filenames protect the references against reordering.
        // Separate cursor references expose style-specific regressions that
        // a single final screenshot of the same terminal would conceal.
        let number = if index == 0 { 1 } else { index + 2 };
        let file = format!("{number:03}-{name}.png");
        let actual = Path::new("target/harness/terminal-frame").join(&file);
        // The asset path is stable across hosts, unlike the temporary
        // render directory whose contents are recreated on every run.
        let expected = Path::new("assets/scenarios/terminal-frame").join(&file);
        if bless {
            // Only these image candidates enter the review set. They stay
            // pending until a human inspects the colored underlines and ink.
            // The updater never copies grid dumps or timing output, since
            // those are useful diagnostics but not pixel references.
            fs::create_dir_all("assets/scenarios/terminal-frame")
                .map_err(|error| error.to_string())?;
            fs::copy(&actual, &expected).map_err(|error| error.to_string())?;
            if verbose {
                println!("BLESSED {}", expected.display());
            }
        } else {
            // The fixed ±2-byte, 0.1% comparator is shared with M3 and M4;
            // no new M6 tolerance is introduced for curly Slug geometry.
            compare(&actual, &expected)?;
            if verbose {
                println!("PASS golden terminal {name}");
            }
        }
    }
    Ok(())
}

/// Compares the M4 backend-seam specimens with their pending references.
pub(crate) fn run_text(bless: bool, verbose: bool) -> Result<(), String> {
    // The scenario's pixel/ink assertions run before any PNG is copied or
    // compared, so a blank or broken join cannot become a valid reference.
    crate::commands::quiet(
        "cargo",
        &[
            "run",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/text.pzs",
        ],
    )?;
    for (index, name) in [
        "regular-1x",
        "bold-1x",
        "italic-1x",
        "ligatures-off",
        "symbol-and-tofu",
        "grid-1x",
        "regular-2x",
        "grid-2x",
        "arc-dash-2x",
        "arc-dash-1x",
        "bold-2x",
        "italic-2x",
        "ligatures-off-2x",
        "symbol-and-tofu-2x",
    ]
    .iter()
    .enumerate()
    {
        // The numeric prefix is part of the harness evidence contract;
        // a reordered screenshot would otherwise compare the wrong style.
        let file = format!("{:03}-{name}.png", index + 1);
        let actual = Path::new("target/harness/text").join(&file);
        let expected = Path::new("assets/scenarios/text").join(&file);
        if bless {
            // R15 keeps these exact images pending until a reviewer sees
            // their strokes, ligatures, fallback icon and cell joins.
            fs::create_dir_all("assets/scenarios/text").map_err(|error| error.to_string())?;
            // Copy only the fourteen intended screenshots, not log files or
            // a stale extra PNG left by an earlier longer scenario.
            fs::copy(&actual, &expected).map_err(|error| error.to_string())?;
            if verbose {
                println!("BLESSED {}", expected.display());
            }
        } else {
            // Both dimensions and every RGBA pixel go through the fixed
            // §2.7 comparator; no M4-specific tolerance is introduced.
            compare(&actual, &expected)?;
            if verbose {
                println!("PASS golden text {name}");
            }
        }
    }
    Ok(())
}

/// Pins every size, DPR and transform independently of the scenario script.
fn expected_names() -> Vec<String> {
    // A script edit cannot silently remove a reference: this list remains
    // the acceptance contract until the reviewer approves a new matrix.
    let mut names = [
        "small",
        "large",
        "glyph",
        "glyph-large",
        "rotated",
        "sheared",
        "zoomed",
    ]
    .map(str::to_string)
    .to_vec();
    for size in [6, 8, 10, 12, 14, 16, 24, 48, 96] {
        for dpr in [1, 2] {
            names.push(format!("specimen-{size}-{dpr}x"));
        }
    }
    for treatment in ["rot15", "rot90", "shear12", "zoom4"] {
        // Both pixel ratios use the same affine treatment, revealing a
        // Jacobian error that only appears after DPR doubles the matrix.
        for dpr in [1, 2] {
            names.push(format!("specimen-{treatment}-24-{dpr}x"));
        }
    }
    names
}

/// Applies the fixed §2.7 tolerance to two decoded RGBA8 images.
pub(crate) fn compare(actual: &Path, expected: &Path) -> Result<(), String> {
    let actual = png::read(actual)?;
    let expected = png::read(expected)?;
    if actual.width != expected.width || actual.height != expected.height {
        return Err("golden dimensions differ".into());
    }
    // Count affected pixels rather than channels: a colored defect must
    // not be discounted just because only one channel changed strongly.
    let mut beyond = 0usize;
    let mut first = None;
    for (index, (seen, reference)) in actual
        .pixels
        .chunks_exact(4)
        .zip(expected.pixels.chunks_exact(4))
        .enumerate()
    {
        if seen.iter().zip(reference).any(|(a, b)| a.abs_diff(*b) > 2) {
            beyond += 1;
            first.get_or_insert(index);
        }
    }
    let total = actual.width as usize * actual.height as usize;
    if beyond * 1000 > total {
        return Err(format!(
            "golden mismatch: {beyond}/{total} pixels beyond ±2; first pixel {first:?}"
        ));
    }
    Ok(())
}

/// M8 fractional clipping keeps its own two small, pending-review references.
pub(crate) fn run_scroll(bless: bool, verbose: bool) -> Result<(), String> {
    // The script checks clipped glyph ink and every padding pixel before comparison.
    crate::commands::quiet(
        "cargo",
        &[
            "run",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/scroll.pzs",
        ],
    )?;
    for (index, name) in ["fractional-1x", "fractional-2x"].iter().enumerate() {
        // Scenario numbering is stable so each DPR compares its own hinting path.
        let file = format!("{:03}-{name}.png", index + 1);
        let actual = Path::new("target/harness/scroll").join(&file);
        let expected = Path::new("assets/scenarios/scroll").join(&file);
        if bless {
            // R15: blessing only records candidates; human visual review stays pending.
            fs::create_dir_all("assets/scenarios/scroll").map_err(|error| error.to_string())?;
            fs::copy(&actual, &expected).map_err(|error| error.to_string())?;
        } else {
            // Reuse the existing dimensions/pixel tolerance without a scrolling exception.
            compare(&actual, &expected)?;
        }
        if verbose {
            println!("{} golden {name}", if bless { "BLESSED" } else { "PASS" });
        }
    }
    Ok(())
}
