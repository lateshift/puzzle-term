//! D-51 structural scrolling and chronological parser measurements (M16.8).
//!
//! Source recordings come from the user's installed vtebench at each geometry;
//! they are never substituted with smaller invented workloads. Every sample
//! remains in CSV, including cold trials. Partial regions may scale with height,
//! so matched-height isolated probes distinguish that from width-dependent work.
//!
//! The lazy-row oracle exercises the same VT controls with physical eager clears,
//! comparing logical cells, sparse cluster metadata and copied snapshot values.
//! Allocation identity and untouched retired storage provide a separate proof
//! that an apparent speedup is actually avoiding full-width cold writes.
//! Pointer-size tests guard the representation directly: a timing improvement
//! alone cannot prove that region movement no longer copies large row records.
//! Reset reuse tests retire tracked identities, verify bounded inactive storage
//! and cross geometry changes; retained capacity is never retained document text.
//! Corpus files are geometry-specific because their generators query the PTY.
//! Only the plain one-character/LF case has identical written work at both widths.
//! Full-width text legitimately writes more cells in a wider terminal, while
//! partial regions legitimately move more handles in a taller terminal.
//! Isolating equal-height region bands prevents either effect being mislabeled
//! as a width-dependent scan. Empty and populated histories both run through
//! physical deque wrap, keeping a history-length dependency visible in evidence.
//!
//! Reset observations time RIS, setup and ordinary output separately: retiring
//! a populated history must not be mistaken for slow parsing of the next corpus.
//! The first trial retains allocation growth; later trials reveal whether that
//! same bounded storage is reused. Live and inactive row bytes share one ceiling.
//! These costs are observations, not a new reset-duration performance budget.
//! Functional tests independently prove that reused storage cannot expose text
//! or tracked identities from the document that the user explicitly cleared.
//!
//! Deterministic operation counts, approved 2026-10-09, are the width gate:
//! every scroll touches zero cells and moves bounded row owners independently
//! of column count. Timing samples remain chronological diagnostic evidence.
//! Earlier pooled median-versus-maximum results remain in M16 evidence; they
//! are no longer acceptance thresholds. Timings still require complete valid
//! samples so malformed observations cannot silently pass as useful evidence.
//!
//! The structural witnesses observe cell stores and movement operands, rather
//! than predicting work from the requested region. An eager-clear positive
//! control proves that the zero-cell assertion would detect the original bug.
//! Both heights exercise disabled history and a saturated history ring; rows
//! contain stored ink before counting, so blank setup cannot conceal clearing.
//! Wrapped regions also count every exchange, including repeated pivot moves,
//! preventing distinct-owner deduplication from concealing excessive swaps.
//! The timing collector retains each chronological width pair separately.
//! Reporting an old wide-median/narrow-maximum comparison remains useful for
//! diagnosis, but scheduler spread does not prove whether cells were touched.
use super::super::{m7::test, report};
use std::{fs, path::PathBuf, process::Command};

/// The full comparison includes ordinary text and all six scrolling workloads.
const CORPORA: [&str; 10] = [
    // Plain text plus every scrolling workload belong to the parity inventory.
    "unicode",
    "medium_cells",
    "sync_medium_cells",
    "dense_cells",
    "scrolling",
    "scrolling_bottom_region",
    "scrolling_bottom_small_region",
    "scrolling_top_region",
    "scrolling_top_small_region",
    "scrolling_fullscreen",
];

/// Adds functional storage proofs and fresh, optimized geometry observations.
pub(super) fn run(passed: &mut bool) {
    // Each storage proof runs independently, so failures cannot hide later criteria.
    for name in [
        // Every public reader must agree with physically cleared reference rows.
        "row::cells::tests::random_vt_edits_match_eager_rows",
        // Cold suffix contents remain untouched while BCE and snapshot copies stay correct.
        "row::cells::tests::recycle_leaves_cold_cells_untouched_and_gap_uses_bce",
        "row::cells::tests::snapshot_copy_never_materializes_or_copies_the_implicit_suffix",
        // Eight-byte owners preserve row addresses and allocate nothing when warm.
        "ring::tests::region_handles_keep_survivors_at_stable_addresses",
        // Explicit history deletion retires identities without rebuilding warmed capacity.
        "ring::tests::cleared_history_reuses_storage_without_allocations",
        "ring::tests::inactive_history_obeys_limits_geometry_and_bce",
        // Real RIS and ED3 preserve warmed grapheme stores, not old buffer contents.
        "terminal::tests::repeated_reset_and_clear_reuse_clustered_rows",
        // History serialization must not carry a prior row's colored BCE forward.
        "view::tests::serialized_history_starts_new_rows_with_default_bce",
        // Count actual storage operations at both widths, heights and history states.
        "ring::tests::operation_counts::scroll_counts_are_zero_cells_bounded_handles_and_width_independent",
        // Prove the observer detects the full-width clearing this gate forbids.
        "ring::tests::operation_counts::scroll_counter_detects_eager_clearing_and_cell_copying",
    ] {
        report(&format!("M16.8 {name}"), test("puzzle-vt", name), passed);
    }
    report(
        "M16.8 all ten headless corpora, two geometries; timings informational",
        corpora(),
        passed,
    );
    report(
        "M16.8 matched-height region timing observations, no timing threshold",
        regions(),
        passed,
    );
    report(
        "M16.8 reset/setup/feed costs and bounded reused storage, two geometries",
        resets(),
        passed,
    );
}

/// Explicit reset costs remain separate from ordinary parsing and carry no new ceiling.
fn resets() -> Result<(), String> {
    // The same recording feeds all phases; no benchmark pattern reaches core code.
    build("reset_bench")?;
    for (columns, rows) in [(120, 40), (282, 63)] {
        let source = sources()
            .join(format!("{columns}x{rows}"))
            .join("scrolling_fullscreen");
        let destination = format!("target/m16-8/accept/reset-{columns}x{rows}.csv");
        execute(
            "reset_bench",
            &[
                // Pass each phase's original captured bytes and explicit geometry.
                source.join("setup").display().to_string(),
                source.join("benchmark").display().to_string(),
                columns.to_string(),
                rows.to_string(),
                destination.clone(),
            ],
        )?;
        // Require every chronological trial, including the first allocation growth.
        let csv = fs::read_to_string(destination).map_err(|e| e.to_string())?;
        if csv.lines().count() != 31 {
            return Err("ten complete reset/setup/feed trials absent".into());
        }
        for line in csv.lines().skip(1) {
            // These owned diagnostic fields contain no escaped or quoted commas.
            let fields: Vec<_> = line.split(',').collect();
            if fields.len() != 7 {
                return Err("malformed reset stage sample".into());
            }
            // Inactive allocations share the existing history byte ceiling.
            let bytes: usize = fields[6].parse().map_err(|_| "invalid retained bytes")?;
            if bytes > 32 * 1024 * 1024 {
                return Err("live plus inactive history exceeds existing byte budget".into());
            }
            // Reset/setup costs are reported, not compared with an invented bound.
        }
    }
    Ok(())
}

/// Builds the ordinary diagnostic examples before starting any measurement clock.
fn build(example: &str) -> Result<(), String> {
    // Release defines native §15 timing; compilation finishes before observation.
    crate::commands::quiet(
        "cargo",
        &[
            "build",
            "--release",
            "-p",
            "puzzle-term",
            "--example",
            example,
        ],
    )
}

/// Keeps local recordings configurable without fetching or vendoring third-party assets.
fn sources() -> PathBuf {
    // Missing setup or corpus data propagates as FAIL, never an empty benchmark.
    std::env::var_os("PUZZLE_SCROLL_CORPORA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/m16-8/corpora"))
}

/// Runs one caller-selected executable with explicit paths and no shell interpretation.
fn execute(example: &str, args: &[String]) -> Result<(), String> {
    // Argument vectors preserve spaces and literal punctuation in supplied paths.
    let status = Command::new(format!("target/release/examples/{example}"))
        .args(args)
        .status()
        .map_err(|e| e.to_string())?;
    // A crashed executable cannot contribute a partially written passing receipt.
    if !status.success() {
        return Err(format!("{example} measurement failed"));
    }
    Ok(())
}

/// All ten corpora are timed through both VT and Session at both requested geometries.
fn corpora() -> Result<(), String> {
    // Build once before any optimized observations start.
    build("scrolling_bench")?;
    let out = PathBuf::from("target/m16-8/accept");
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let mut plain = [Vec::new(), Vec::new()];
    for name in CORPORA {
        // The approved structural protocol interleaves three width pairs.
        // Other corpora keep their five VT and five Session observations.
        let pairs = if name == "scrolling" { 3 } else { 1 };
        for pair in 0..pairs {
            for (width, (columns, rows)) in [(120, 40), (282, 63)].into_iter().enumerate() {
                let samples = corpus_sample(name, columns, rows, pair, &out)?;
                if name == "scrolling" {
                    // Pool all chronological trials, never a fastest pair.
                    plain[width].extend(samples);
                }
            }
        }
    }
    timing_information("plain LF", &plain[0], &plain[1]);
    println!("M16.8 all samples: target/m16-8/accept/*.csv; matched host parity remains REVIEW");
    Ok(())
}

/// Records one geometry/pair through the same original setup and workload bytes.
fn corpus_sample(
    name: &str,
    columns: usize,
    rows: usize,
    pair: usize,
    out: &std::path::Path,
) -> Result<Vec<f64>, String> {
    let source = sources().join(format!("{columns}x{rows}")).join(name);
    // Distinct pair paths preserve every five-trial observation, including cold growth.
    let destination = out.join(format!("{columns}x{rows}-{name}-pair-{pair}.csv"));
    execute(
        "scrolling_bench",
        &[
            source.join("setup").display().to_string(),
            source.join("benchmark").display().to_string(),
            columns.to_string(),
            rows.to_string(),
            destination.display().to_string(),
        ],
    )?;
    let csv = fs::read_to_string(destination).map_err(|e| e.to_string())?;
    // Five VT and five Session observations are mandatory in every pair.
    if csv.lines().count() != 11 {
        return Err("ten chronological VT/Session samples absent".into());
    }
    session_lines(&csv)
}

/// Extracts the Session's five finite, positive per-line costs from a fresh probe.
fn session_lines(csv: &str) -> Result<Vec<f64>, String> {
    // Pure-VT observations stay in the CSV for stage attribution.
    let mut samples = Vec::new();
    // Our CSV fields are numeric and cannot contain quoted commas.
    for line in csv.lines().skip(1) {
        let fields: Vec<_> = line.split(',').collect();
        // Reject a changed schema before indexing its timing column.
        if fields.len() != 8 {
            return Err("malformed headless stage sample".into());
        }
        // This owner includes reconciliation and scheduling absent from pure VT.
        if fields[0] == "session" {
            let value: f64 = fields[7].parse().map_err(|_| "invalid per-line cost")?;
            // NaN or a negative clock must never lower a measured distribution.
            if !value.is_finite() || value <= 0.0 {
                return Err("invalid positive cost".into());
            }
            samples.push(value);
        }
    }
    // The possibly colder first trial is required along with the warmed four.
    if samples.len() != 5 {
        return Err("five Session observations absent".into());
    }
    Ok(samples)
}

/// Reports the old timing comparison without treating scheduler spread as structure.
fn timing_information(cohort: &str, narrow: &[f64], wide: &[f64]) {
    // Keep raw chronology in CSV, sorting only this reporting copy.
    let mut sorted = wide.to_vec();
    sorted.sort_by(f64::total_cmp);
    let maximum = narrow.iter().copied().fold(0.0, f64::max);
    println!(
        "M16.8 INFO {cohort}: wide median {} / narrow maximum {maximum}; timings only",
        sorted[sorted.len() / 2]
    );
}

/// Equal-height probes cover both deque fragments and empty/populated history.
fn regions() -> Result<(), String> {
    // Remove scalar parsing from this observation of eight-byte handle movement.
    build("region_bench")?;
    for rows in [40, 63] {
        let mut narrow = Vec::new();
        let mut wide = Vec::new();
        // Three interleaved pairs retain all five trials of every path/history.
        for pair in 0..3 {
            narrow.push(region_sample(120, rows, pair)?);
            wide.push(region_sample(282, rows, pair)?);
        }
        for cohort in 0..4 {
            // Each five-trial cohort describes one path and one history state.
            // Combine only like cohorts; preserve all original CSV chronology.
            let pooled = |pairs: &[Vec<f64>]| -> Vec<f64> {
                pairs
                    .iter()
                    .flat_map(|samples| samples[cohort * 5..cohort * 5 + 5].iter().copied())
                    .collect()
            };
            timing_information(
                &format!("region rows={rows} cohort={cohort}"),
                &pooled(&narrow),
                &pooled(&wide),
            );
        }
    }
    Ok(())
}

/// Measures one width in an interleaved pair without overwriting any earlier result.
fn region_sample(columns: usize, rows: usize, pair: usize) -> Result<Vec<f64>, String> {
    let destination = format!("target/m16-8/accept/regions-{columns}x{rows}-pair-{pair}.csv");
    execute(
        "region_bench",
        &[
            columns.to_string(),
            rows.to_string(),
            "100000".into(),
            destination.clone(),
        ],
    )?;
    let csv = fs::read_to_string(destination).map_err(|e| e.to_string())?;
    // The representation and schema headers precede four five-trial cohorts.
    let samples: Vec<f64> = csv
        .lines()
        .skip(2)
        .map(|line| {
            line.rsplit(',')
                .next()
                .ok_or("missing region cost")?
                .parse::<f64>()
                .map_err(|_| "invalid region cost")
        })
        .collect::<Result<_, _>>()?;
    if samples.len() != 20 || samples.iter().any(|x| !x.is_finite() || *x <= 0.0) {
        return Err("twenty chronological region samples absent".into());
    }
    Ok(samples)
}
