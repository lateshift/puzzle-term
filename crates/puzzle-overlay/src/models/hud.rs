//! Frozen debug data carries no timer, observer or platform object.
/// Last submitted frame's values, copied only at source publication boundaries.
#[derive(Clone, Copy, Default)]
pub struct Hud {
    /// Hidden by default; only an explicit debug host request enables observation.
    pub shown: bool,
    /// Total measured CPU build time, excluding presentation and acquire waits.
    pub cpu_ms: f64,
    /// Actual consumed bytes per wall second between completed frame observations.
    pub parse_bytes_per_second: f64,
    /// Rectangle, vector and bitmap instance counts in pipeline order.
    pub instances: [usize; 3],
    /// Combined allocated glyph atlas capacities.
    pub glyph_bytes: usize,
    /// Submitted upload data in bytes, including GPU-only raster copies.
    pub upload_bytes: usize,
}
