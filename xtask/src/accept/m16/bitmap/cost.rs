//! Rasterization reports per-glyph, curve and physical sample cost without a fixed limit.
//!
//! The developer binary pre-extracts and converts all outlines before its raster
//! clocks. Its primary-font calibration remains a separate preparation measurement.
//! CPU samples evaluate Slug coverage; GPU samples enqueue production-shader draws
//! and atlas copies, then measure completed wall time without readback. This is not
//! an exclusive shader timestamp. All cold first samples remain in the CSV.
//! Acceptance requires both producers at both DPRs and every named test glyph.
//! It verifies positive finite measurements and their normalization denominators,
//! rather than comparing a pixel-area stage to the outline per-curve ceiling.
//! CSV columns retain glyph identity, DPR, producer, original curve count, sample
//! ordinal, device crop area, enqueue cost, completed cost and both normalizations.
//! Zero is permitted only for the first sample ordinal; it is not a valid cost,
//! curve count or crop area. Rounded output needs a small decimal-rounding allowance
//! when its quotients are checked, but no timing threshold is relaxed by that check.
//! The full 21-sample series includes initialization, so an empty or truncated file
//! cannot pass simply because its surviving rows are individually well formed.
//! This wrapper retains the producer's stderr with stdout to preserve GPU failures.
use std::{collections::HashMap, fs, process::Command};

/// Runs the current release producer and rejects incomplete or malformed cost evidence.
pub(super) fn run() -> Result<(), String> {
    // Raster observations require the same checked feature build as live windows.
    // A failed prerequisite reports failure without launching an older producer.
    super::super::complete::release()?;
    let output = Command::new("target/release/puzzle-desktop")
        .arg("grid-raster-bench")
        .output()
        .map_err(|e| e.to_string())?;
    // Both calibration and raster samples remain in the original chronological output.
    let mut log = output.stdout;
    log.extend_from_slice(&output.stderr);
    fs::write("target/m16-4/raster-cost.log", log).map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("raster cost producer failed; target/m16-4/raster-cost.log".into());
    }
    let csv = fs::read_to_string("target/m16-4/raster-cost.csv").map_err(|e| e.to_string())?;
    let mut groups = HashMap::new();
    for row in csv.lines().skip(1) {
        let columns: Vec<_> = row.split(',').collect();
        if columns.len() != 10 {
            return Err("raster cost row has wrong column count".into());
        }
        // Normalized observations must refer to real curves and real generated device pixels.
        let n = numeric(&columns)?;
        if (n[7] / n[3] - n[8]).abs() > 0.000002 || (n[7] / n[5] - n[9]).abs() > 0.000002 {
            return Err("raster cost normalization is inconsistent".into());
        }
        let key = (columns[0], columns[1], columns[2]);
        *groups.entry(key).or_insert(0) += 1;
    }
    // Eight glyphs × two DPRs × two producers, including all 21 cold sample batches.
    if groups.len() != 32 || groups.values().any(|count| *count != 21) {
        return Err("CPU/GPU DPR 1/2 raster sample cohort incomplete".into());
    }
    println!(
        "M16.4 raster cost: 672 samples; CPU/GPU µs per glyph/curve/device pixel; no readback or new fixed limit; target/m16-4/raster-cost.csv"
    );
    Ok(())
}

/// Parses cost columns while permitting the first chronological sample index to be zero.
fn numeric(columns: &[&str]) -> Result<Vec<f64>, String> {
    // Non-numeric identity/path columns remain part of the cohort key above.
    let mut n = vec![0.0; 10];
    for i in [1, 3, 4, 5, 6, 7, 8, 9] {
        let value: f64 = columns[i]
            .parse()
            .map_err(|_| "invalid raster cost number")?;
        if !value.is_finite() || (i != 4 && value <= 0.0) || value < 0.0 {
            return Err("nonpositive/nonfinite raster cost observation".into());
        }
        n[i] = value;
    }
    Ok(n)
}
