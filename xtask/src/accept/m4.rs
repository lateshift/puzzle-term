//! M4 acceptance for fonts, text placement and pack-first startup (§17).
//!
//! Focused tests name each boundary so a failure cannot hide behind one
//! aggregate Cargo status: font parsing and fallback, shaped clusters,
//! D-27 hinting, the D-28 backend seam, and hostile pack decoding.
//! Native harness scenes then exercise actual Metal text, icon and box ink
//! at 1× and 2× with exact shared-edge pixel assertions. The web build
//! validates the WebGPU-only pack scene and the release size budget.
//!
//! All criteria continue after failure. Human sharpness and reference
//! quality stay separate R15 review lines, even when mechanical checks
//! pass. The command does not claim a browser screenshot it cannot see.
//!
//! The font-source criterion checks bytes on disk and functional fallback;
//! a successful TTF parse alone would not reveal an icon mapped to .notdef.
//! The shaping criterion needs both a direct ASCII path and Harfrust: the
//! former avoids needless OpenType work when ligatures are disabled, while
//! the latter supplies coding substitutions. Cursor splitting is checked
//! separately because a ligature under the cursor harms editing.
//!
//! The backend criterion protects placement ownership. It checks that an
//! atlas request for an effect becomes a vector draw without modifying the
//! glyph origin or advance. D-27's test uses real fitted control points at
//! two ppems, so a cache key missing ppem cannot pass by accident.
//!
//! The pack checks have three layers. The decoder rejects hostile bytes;
//! the generator enforces the four-file brotli limit from current source
//! fonts; the native GPU probe consumes a pack without opening any TTF and
//! counts drawn ink. The browser build checks that Trunk copied the pack
//! beside wasm. These checks catch distinct packaging and rendering bugs.
//!
//! The text and regenerated M3 golden checks compare pixels only after
//! their scenario assertions. A matching stale PNG cannot stand in for a
//! fresh draw. The M3 geometry gate remains independently runnable and
//! retains its captured-input CPU oracle from the earlier milestone.
//! The release CPU probe enforces the user's four 240-cell timing limits
//! and first-frame pack/hinting limits. A separate allocation-counter test
//! protects the P1 guarantee for rows after their buffers have warmed.
use super::report;
use std::{collections::BTreeSet, path::Path};

/// Release CPU and allocation budgets approved for M4 rows.
mod budgets;
/// Pixel-profile checks for every declared solid join and dashed tile.
mod geometry;

/// Runs a specifically named behaviour test and rejects zero test matches.
fn text_test(name: &str) -> Result<(), String> {
    // `--exact` prevents a similarly named regression from masking the
    // deletion of the contract test this criterion relies on.
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-text", "--lib", name, "--", "--exact"],
    )?;
    // Cargo succeeds when a filter matches no tests. Require this named
    // assertion in stdout so a renamed test cannot make acceptance green.
    if !output.contains(&format!("test {name} ... ok")) {
        return Err(format!("puzzle-text test {name} did not run"));
    }
    Ok(())
}

/// Verifies all tests that support one independently reported criterion.
fn tests(names: &[&str]) -> Result<(), String> {
    // Stop within one criterion at its first failed prerequisite while
    // leaving all other criteria to run through the outer report loop.
    for name in names {
        text_test(name)?;
    }
    Ok(())
}

/// Requires the shipped face and the bundled icon/tofu boundary.
fn font_sources() -> Result<(), String> {
    // The regular and italic variable TTFs are separate approved assets;
    // the icon face is the final fallback before the built-in hex box.
    for path in [
        "assets/fonts/JetBrainsMono-Variable.ttf",
        "assets/fonts/JetBrainsMono-Italic-Variable.ttf",
        "assets/fonts/SymbolsNerdFontMono-Regular.ttf",
    ] {
        if !Path::new(path).is_file() {
            return Err(format!("bundled M4 face missing: {path}"));
        }
    }
    tests(&[
        // JetBrains 10/13 px cannot snap caps within the ±3% limit;
        // rejecting a partial nudge keeps D-27's real decision intact.
        "metrics::tests::unsnappable_sizes_stay_unchanged",
        // Preloaded pack and fetched full font share integer cell width,
        // height and baseline at both fractional and whole DPRs.
        "metrics::tests::pack_and_full_font_share_cell_geometry",
        // U+F015 must reach Symbols Nerd Font, while unassigned U+0378
        // must reach tofu rather than a font's missing-glyph outline.
        "font_set::tests::symbols_fallback_and_tofu_boundary",
        "layout::tests::symbol_overflow_requires_a_blank_neighbour",
    ])
}

/// Checks the four-font pack generator and its hostile-input parser.
fn font_packs() -> Result<(), String> {
    tests(&[
        // Round trip checks directory handles against ready-to-upload
        // curve and band planes, not just a magic/version header.
        "pack::tests::round_trip_has_ready_slug_texels",
        // Packs are external bytes on the web, so arbitrary malformed
        // input must produce a report instead of a panic (R8).
        "pack::tests::hostile_pack_never_panics",
    ])?;
    // The generator enforces the 250 KB brotli target on freshly
    // regenerated bytes, so an old committed pack cannot mask growth.
    crate::commands::quiet("cargo", &["xtask", "font-pack"])
}

/// Proves actual GPU upload and ink from pack bytes with no TTF path.
fn pack_gpu() -> Result<(), String> {
    // This binary's pack-only mode has no source-font read path. It proves
    // Slug can consume the precomputed texel planes before TTF fetch.
    let output = crate::commands::capture(
        "cargo",
        &["run", "-p", "puzzle-desktop", "--", "pack-probe"],
    )?;
    let ink = output
        .split("font pack GPU probe: ")
        .nth(1)
        .and_then(|tail| tail.split_whitespace().next())
        .and_then(|count| count.parse::<usize>().ok())
        .ok_or("pack-only GPU probe omitted its ink count")?;
    // A count is checked from this invocation's stdout. A previous PNG
    // left in target/ does not provide evidence of a fresh GPU pass.
    if ink < 20 {
        // An image file left by an earlier run cannot satisfy this check.
        // The probe's own ink threshold also rejects an all-black output.
        return Err("pack-only GPU probe did not report expected ink".into());
    }
    Ok(())
}

/// Builds the pack-backed browser path and checks its shipped budget.
fn web_text() -> Result<(), String> {
    // Trunk compiles wasm and copies page assets. A native pack probe alone
    // cannot establish that the browser fetch URL resolves after bundling.
    let output = crate::commands::capture("cargo", &["xtask", "web", "build"])?;
    let value = output
        .split("brotli=")
        .nth(1)
        .and_then(|part| part.split_whitespace().next())
        .ok_or("web build omitted brotli size")?;
    let bytes: usize = value.parse().map_err(|_| "invalid wasm size")?;
    // The 1 MB target applies to compressed wasm, not the separate TTFs
    // or font packs. Raising this ceiling would require R5 approval.
    if bytes > 1_000_000 {
        return Err(format!("M4 wasm is {bytes} B brotli, above 1 MB"));
    }
    // The pack has to travel beside the wasm. Merely compiling its Rust
    // decoder would not prove the browser can fetch startup text bytes.
    for name in ["regular", "bold", "italic", "bold-italic"] {
        // All style slots must be available for first-frame text even
        // though the fixed browser review line uses the regular slot.
        let path = format!("dist/web/jetbrains-{name}.pzfp");
        if !Path::new(&path).is_file() {
            return Err(format!("Trunk omitted the M4 font pack: {path}"));
        }
    }
    Ok(())
}

/// Proves Harfrust and skrifa share a single read-fonts version.
fn font_dependency_version() -> Result<(), String> {
    // Harfrust and skrifa both parse OpenType tables through read-fonts;
    // two minor versions would duplicate parser code in browser wasm.
    let tree = crate::commands::capture("cargo", &["tree", "-p", "puzzle-text"])?;
    let mut versions = BTreeSet::new();
    for line in tree.lines() {
        // Cargo can print the same package at several positions in the
        // dependency tree; the set counts distinct versions only.
        if let Some(tail) = line.split("read-fonts v").nth(1)
            && let Some(version) = tail.split_whitespace().next()
        {
            versions.insert(version.to_owned());
        }
    }
    if versions.len() != 1 || !tree.contains("harfrust v") || !tree.contains("skrifa v") {
        return Err(format!(
            "font dependency versions are not aligned: {versions:?}"
        ));
    }
    Ok(())
}

/// Reports focused font and shaping guarantees independently.
fn report_unit_criteria(passed: &mut bool) {
    // Each line corresponds to one M4 behaviour family in the §17 row.
    // The workspace gate below already covers the complete unit suite;
    // these focused calls make regressions attributable to a subsystem.
    report(
        "bundled faces, metrics and icon/tofu fallback",
        font_sources(),
        passed,
    );
    report(
        "ASCII path, Harfrust ligatures and bounded run cache",
        // The pair distinguishes direct cmap lookup from contextual
        // substitutions and confirms the exact-key cache's reuse.
        tests(&[
            "shape::tests::ascii_direct_path_preserves_cluster_bytes",
            "shape::tests::ligatures_use_harfrust_and_cache_the_run",
            "layout::tests::layout_respects_font_feature_and_builtin_choices",
        ]),
        passed,
    );
    report(
        "cursor break and backend-neutral grid placement",
        // A cursor inside `->` must reveal editable source characters
        // without shifting the later fixed-cell origin.
        text_test("layout::tests::cursor_break_and_hinting_preserve_grid"),
        passed,
    );
    report(
        "D-27 light hinting threshold and ppem cache",
        // The display DPR threshold and ppem key solve separate failure
        // modes: fitting where invalid, and reusing the wrong fitted shape.
        tests(&[
            "hinting::tests::eligibility_boundaries",
            "font_set::tests::hinted_shape_keeps_grid_placement",
        ]),
        passed,
    );
    report(
        "D-28 backend invariance and effect vector selection",
        // Effects must select vectors even when a future atlas is requested;
        // placement remains the authority for cursor/selection geometry.
        tests(&[
            "glyph::tests::effect_selects_vector_without_moving_placement",
            "glyph::tests::auto_uses_slug_for_small_grid_text",
        ]),
        passed,
    );
    report(
        "builtin geometry covers box, block, Braille, Powerline and tofu",
        // Range coverage catches a missing procedural path, while the
        // boundary test detects a one-pixel gap between adjacent cells.
        tests(&[
            "builtin::tests::assigned_ranges_have_paths",
            "builtin::tests::box_arms_touch_the_cell_edge",
            "builtin::tests::rounded_corners_have_one_half_cell_radius",
            "layout::tests::builtin_cells_and_tofu_follow_the_grid",
        ]),
        passed,
    );
}

/// Reports image equality and the two stronger geometry invariants.
fn report_text_scenes(passed: &mut bool) {
    // Reference equality protects all specimen ink, while pixel profiles
    // catch a bad join even if an old bad reference matches it exactly.
    report(
        "M4 text scenes and 1x/2x box joins",
        crate::golden::run_text(false, false),
        passed,
    );
    // Both geometry lines depend on this fresh render. A failed harness
    // cannot let yesterday's PNGs satisfy an unrelated acceptance line.
    let geometry_run = geometry::render_scenes();
    report(
        "solid grid and arc edge profiles match within 2/255",
        geometry_run
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|()| geometry::strict_joins()),
        passed,
    );
    report(
        "dash and gap lengths tile within one device pixel",
        geometry_run
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|()| geometry::dash_uniformity()),
        passed,
    );
}

/// Emits every M4 criterion and the exact R15 evidence locations.
pub(super) fn run() -> bool {
    // `report` records failure without returning, so a broken browser
    // build still leaves native font and GPU diagnostics on this run.
    let mut passed = true;
    report(
        "native and wasm check",
        crate::commands::check(),
        &mut passed,
    );
    report_unit_criteria(&mut passed);
    report(
        "single read-fonts version for skrifa and harfrust",
        // The version check is structural evidence, separate from a
        // passing wasm compile that could still contain both parsers.
        font_dependency_version(),
        &mut passed,
    );
    report(
        "four unhinted packs validate and total <= 250 KB brotli",
        // Regeneration catches a stale checked-in pack; D-27 hinted data
        // stays runtime-only rather than inflating these shipped files.
        font_packs(),
        &mut passed,
    );
    report(
        "pack-only GPU first frame without a TTF",
        // Decoding alone cannot expose a wrong band-header texture address;
        // this producer counts real Metal output pixels.
        pack_gpu(),
        &mut passed,
    );
    report_text_scenes(&mut passed);
    report(
        "hinted M3 specimens and transformed/unhinted references",
        // Only the nine normal 1× scenes changed for D-27. The unchanged
        // affine and 2× scenes remain regression guards for true outlines.
        crate::golden::run(false, false),
        &mut passed,
    );
    budgets::report_shape_budgets(&mut passed);
    report(
        "web pack scene builds under 1 MB wasm",
        // The first browser frame uses a copied .pzfp beside the wasm;
        // actual Chrome/Safari presentation remains a review item.
        web_text(),
        &mut passed,
    );
    review_items();
    passed
}

/// Human reviewers inspect ink and prose after all mechanical reports.
fn review_items() {
    // These paths are printed even after a mechanical failure. A reviewer
    // can inspect the exact pending screenshots once that failure is fixed.
    println!(
        "REVIEW 1x ThinkPad and browser text: target/harness/text/001-regular-1x.png; ?scene=text; README.md steps"
    );
    println!(
        "REVIEW 2x Retina text: target/harness/text/007-regular-2x.png; 011-bold-2x.png; 012-italic-2x.png"
    );
    println!(
        "REVIEW repaired corners and dashes (confirmed by Claude 2026-09-29): target/harness/text/009-arc-dash-2x.png; 010-arc-dash-1x.png"
    );
    println!(
        "REVIEW repaired references (confirmed by Claude 2026-09-29): assets/scenarios/text/006-grid-1x.png; 008-grid-2x.png; 009-arc-dash-2x.png; 010-arc-dash-1x.png"
    );
    println!(
        "REVIEW other M4 references: assets/scenarios/slug/008..024; assets/scenarios/text/001..014; README.md status"
    );
    println!(
        "REVIEW row-optimization comment quality: crates/puzzle-text/src/layout.rs; crates/puzzle-text/src/face.rs; xtask/src/m4_bench/shaping.rs; README.md status"
    );
}
