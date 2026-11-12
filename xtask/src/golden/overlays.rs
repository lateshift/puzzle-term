//! Bundled-font M9 candidates share the established pixel comparator (§8.1, R15).
//!
//! Machine-specific native CJK and sbix review images are deliberately outside
//! this list. They depend on installed system fonts, so no golden command reads
//! or blesses them. The three public providers exercise the production compiler.
//! Their scripts assert ink before any candidate can be copied into references.
//! Exact names pin two device scales independently of screenshot discovery.
//! The user approved these desktop references on 2026-10-10 (R15).
//! Model scenes also assert authored panel, pill and track colors independently.
use std::{fs, path::Path};

/// Renders every M9.2 bundled scene before comparing or recording its candidates.
/// The fixed comparator checks dimensions, ±2 channel bytes and 0.1% pixels.
/// No overlay-specific tolerance or machine-font exception is introduced here.
/// Copy only declared PNGs; diagnostics and accidental extra screenshots stay local.
pub(crate) fn run(bless: bool, verbose: bool) -> Result<(), String> {
    for (family, labels) in scenes() {
        // Fresh provider output forbids stale target images from accepting a change.
        crate::commands::quiet(
            "cargo",
            &[
                "xtask",
                "harness",
                &format!("assets/scenarios/{family}.pzs"),
            ],
        )?;
        for (index, label) in labels.iter().enumerate() {
            let file = format!("{:03}-{label}.png", index + 1);
            let actual = Path::new("target/harness").join(family).join(&file);
            let reference = Path::new("assets/scenarios").join(family).join(file);
            if bless {
                // The caller viewed these candidates; R15 still requires human approval.
                fs::create_dir_all(reference.parent().ok_or("reference parent absent")?)
                    .map_err(|e| e.to_string())?;
                fs::copy(&actual, &reference).map_err(|e| e.to_string())?;
            } else {
                // Every scene uses the earlier milestones' unchanged numerical rule.
                super::compare(&actual, &reference)?;
            }
            if verbose {
                println!(
                    "{} golden {family} {label}",
                    if bless { "BLESSED" } else { "PASS" }
                );
            }
        }
    }
    Ok(())
}

/// Explicit approved screenshot order excludes stale and diagnostic files.
fn scenes() -> impl Iterator<Item = (&'static str, &'static [&'static str])> {
    [
        // Dense SVG/stroke chunks share the same packing and clipping contract.
        ("overlay-shapes", &["shapes-1x", "shapes-2x"][..]),
        // Affine free text retains bundled fallback faces without platform lookup.
        ("overlay-text", &["text-1x", "text-2x"]),
        // The source grid remains unchanged while each effect alters its ink.
        ("overlay-effects", &["effects-1x", "effects-2x"]),
        // Declarative definitions cover every placement through the public compiler.
        ("overlay-decorations", &["decorations-1x", "decorations-2x"]),
        // Search, scrollbar and pill draw from immutable Session model publications.
        ("overlay-models", &["models-1x", "models-2x"]),
        // Padding and grid-slot scissors have independent authored color assertions.
        ("overlay-core", &["core-1x", "core-2x"]),
        // Cursor suggestions use the ordinary annotation source, never terminal cells.
        ("overlay-ghost", &["ghost-1x", "ghost-2x"]),
        // Provisional composition takes priority over suggestion ink.
        ("overlay-preedit", &["preedit-1x", "preedit-2x"]),
        // Continuations are derived from the effective binding table.
        ("overlay-leader", &["leader-1x", "leader-2x"]),
        // The searchable sheet keeps source-layer labels visible.
        ("overlay-help", &["help-1x", "help-2x"]),
        // Grouped links include both parsed OSC and detected URI hover.
        ("overlay-links", &["links-1x", "links-2x"]),
        // Hover adds copy controls to the same parsed command interval.
        (
            "overlay-blocks",
            &[
                "blocks-1x",
                "blocks-hover-1x",
                "blocks-2x",
                "blocks-hover-2x",
            ],
        ),
        // Bell disappearance is separate from the persistent progress strip.
        (
            "overlay-status",
            &[
                "status-bell-1x",
                "status-progress-1x",
                "status-bell-2x",
                "status-progress-2x",
            ],
        ),
        // Fixed host samples pin layout; real timing tests retain their budgets.
        ("overlay-hud", &["hud-1x", "hud-2x"]),
    ]
    .into_iter()
}
