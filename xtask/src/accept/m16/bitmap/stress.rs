//! Maximized Unicode measures complete frames, cache residency and the warm-up curve.
//!
//! Five-second bins retain the cold prefix rather than selecting a faster interval.
//! The unchanged vector and raster allowances are checked independently. Covered
//! glyph completeness, aborted builds and child keys use the established D-45 gate.
//! Retained reusable entries with no later raster eviction establish that the
//! observed corpus stays cached; printed rates show its steady presentation cost.
//! The reference terminal's external compositor evidence and benchmark table stay separate from
//! Puzzle's own submission timestamps. User visual parity remains a review item.
//!
//! A cache record contains reusable glyph count, physical page capacity and total
//! page replacements. Counts may grow when a later frame first exposes a glyph,
//! but cannot shrink or churn while keeping the same reserved byte capacity.
//! All build attempts participate in the memory check, even unpresented attempts.
//! The thirty-second producing interval excludes the following frozen Hangul tail;
//! blink presentations there cannot inflate Unicode stress throughput.
//! A slower cold prefix stays in the first bin. Every later five-second bin must
//! reach at least that prefix's rate; no new absolute Unicode fps floor is imposed.
//! The retention check covers the entire final producing bin rather than picking a
//! favorable final frame. Failed observations return an error after raw JSON was
//! saved by the window owner, preserving the evidence for diagnosis and review.
use serde_json::Value;

/// Runs a fresh full-corpus window instead of accepting an old artifact's verdict.
pub(super) fn run() -> Result<(), String> {
    let value = super::super::complete::maximized_unicode()?;
    // First-frame covered ink and all sampled GPU crops are required by the existing gate.
    if value["unicode_stress_pass"] != true || value["frozen"]["pass"] != true {
        return Err("maximized Unicode completeness, key or aborted-build gate failed".into());
    }
    let submissions = value["submission_ms"]
        .as_array()
        .ok_or("submission curve absent")?;
    let bins: Vec<f64> = (0..6)
        .map(|bin| {
            // Count chronological submissions in the actual thirty-second producing interval.
            submissions
                .iter()
                .filter(|t| {
                    t.as_f64().is_some_and(|t| {
                        t >= f64::from(bin * 5000) && t < f64::from((bin + 1) * 5000)
                    })
                })
                .count() as f64
                / 5.0
        })
        .collect();
    let late = cache_tail(&value)?;
    // A retained corpus keeps its pages; newly encountered glyphs can occupy unused shelves.
    let first = late.first().ok_or("warm cache tail absent")?;
    // Physical capacity and eviction counts must be constant; retained entries cannot disappear.
    if late
        .iter()
        .any(|cache| cache[1] != first[1] || cache[2] != first[2])
        || late.windows(2).any(|pair| pair[1][0] < pair[0][0])
        || bins[1..].iter().any(|fps| *fps < bins[0])
    {
        return Err(format!(
            "Unicode cache/rate did not stabilize; fps bins={bins:?}"
        ));
    }
    if value["vector_bytes"]
        .as_u64()
        .is_none_or(|bytes| bytes > 48 * 1024 * 1024)
    {
        return Err("vector capacity exceeded 48 MiB".into());
    }
    // Store the summary beside full raw JSON; a compact report never replaces frame attempts.
    std::fs::create_dir_all("target/m16-4").map_err(|e| e.to_string())?;
    let summary: Value = serde_json::json!({"fps_bins":bins,"warm_cache_first":first,"warm_cache_last":late.last(),"fps":value["fps"],"key_ms":value["key_to_child_ms"],"worst_frame_ms":value["worst_frame_ms"]});
    std::fs::write("target/m16-4/maximized-summary.json", summary.to_string())
        .map_err(|e| e.to_string())?;
    println!(
        "M16.4 maximized Unicode: {summary}; reference curve: docs/history/M16.md; visual parity pending"
    );
    Ok(())
}

/// Validates every physical cache record and returns the complete final producing bin.
/// Newly exposed glyphs may add entries, but the caller rejects any loss or page churn.
/// Typed integers keep missing or malformed diagnostic fields from passing comparison.
fn cache_tail(value: &Value) -> Result<Vec<[u64; 3]>, String> {
    let attempts = value["frame_gates"]["attempts"]
        .as_array()
        .ok_or("cache chronology absent")?;
    let mut late = Vec::new();
    for attempt in attempts {
        let record = &attempt["grid_cache_entries_bytes_evictions"];
        // A missing count cannot masquerade as a stable cache with no evictions.
        let cache = [
            record[0].as_u64().ok_or("glyph population absent")?,
            record[1].as_u64().ok_or("raster capacity absent")?,
            record[2].as_u64().ok_or("raster eviction count absent")?,
        ];
        // Count full physical page capacity, including unused shelf space.
        if cache[1] > 48 * 1024 * 1024 {
            return Err("raster capacity exceeded 48 MiB".into());
        }
        // Attempt times share the benchmark origin, independent of successful presentation.
        let started = attempt["start_ms"].as_f64().ok_or("build start absent")?;
        if (25_000.0..30_000.0).contains(&started) {
            late.push(cache);
        }
    }
    Ok(late)
}
