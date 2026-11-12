//! Scalar observations emitted by the real surface executor, without probe policy.
use std::time::Duration;

/// Phase costs measured by the same live frame executor used in ordinary runs.
#[derive(Default, Clone, Copy)]
pub struct FrameWork {
    /// Render-owned row shaping, glyph preparation and assembly of sealed rows.
    pub build: Duration,
    /// Cache/rows/assembly/geometry/overlays/uploads wall durations in that order.
    pub build_phases: [Duration; 6],
    /// Newly exposed or damaged row cache entries rebuilt in this frame.
    pub rebuilt_rows: usize,
    /// Opt-in line metrics, cluster/fallback, shaping/placement and glyph preparation.
    pub row_phases: [u64; 4],
    /// Six cache layers, each recording hits/misses/evictions/hash bytes/scalar keys/preparations.
    pub cache_metrics: [[u64; 6]; 6],
    /// Newly admitted glyph keys; warmed frames cannot perform outline preparation.
    pub glyphs: usize,
    /// Dirty glyph/paint/raster upload records, independent of instance writes.
    pub uploads: usize,
    /// CPU duration of atlas and instance uploads before command encoding.
    pub upload_cpu: Duration,
    /// Upload, encoding, queue submission and present CPU, excluding FIFO acquisition.
    pub submit_cpu: Duration,
}
