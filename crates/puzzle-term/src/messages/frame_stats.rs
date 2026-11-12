//! Opt-in frame statistics are copied values, never a second frame owner.
//!
//! Six coarse phase durations describe CPU construction. Counts and transfer
//! bytes describe the submitted contract, not GPU execution time. No timer runs
//! unless a publication requests observation; hidden HUDs add no observer.
//! Parsing rate uses bytes actually consumed between presented observations,
//! rather than counting offered transport bytes or taking a per-scalar clock.
/// One completed frame's coarse construction and resource measurements.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameStats {
    /// Cache order, rows, assembly, geometry, overlays and uploads, in milliseconds.
    pub cpu_ms: [f64; 6],
    /// Rectangle, Slug and raster instances, in pipeline order.
    pub instances: [usize; 3],
    /// Allocated vector and raster atlas capacities, excluding source font mappings.
    pub glyph_bytes: usize,
    /// CPU-backed texel bytes and GPU-copy payload bytes submitted with this frame.
    pub upload_bytes: usize,
    /// Actual consumed parser bytes per second since the preceding observation.
    pub parse_bytes_per_second: f64,
}
impl FrameStats {
    /// Total construction time retains the sum of independently reported phases.
    pub fn total_cpu_ms(&self) -> f64 {
        self.cpu_ms.iter().sum()
    }
}
