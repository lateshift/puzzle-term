//! Opt-in lookup counts; fixed scalars do not allocate or own cache entries.
//! Counters measure requests rather than net population: an evicted and rebuilt
//! glyph can leave the population unchanged while still doing expensive work.
//! Hosts enable observation explicitly and format snapshots after measurement.

/// Cumulative observations for one cache layer on the frame-building owner.
#[derive(Clone, Copy, Debug, Default)]
pub struct CacheMetrics {
    /// Exact reusable-key matches, after any collision or generation validation.
    pub hits: u64,
    /// Requests that could not reuse an existing result.
    pub misses: u64,
    /// Individually retired entries or pages, according to this layer's unit.
    pub evictions: u64,
    /// UTF-8 bytes hashed for string identities; scalar identities contribute zero.
    pub hashed_bytes: u64,
    /// Reversible scalar identities requiring neither hashing nor retained strings.
    pub scalar_keys: u64,
    /// Cold source preparations; distinguishes actual work from duplicate lookups.
    pub preparations: u64,
}
impl CacheMetrics {
    /// Records a lookup only for an explicitly enabled diagnostic owner.
    pub(crate) fn lookup(owner: &mut Option<Self>, hit: bool, bytes: usize) {
        if let Some(metrics) = owner {
            metrics.hits += u64::from(hit);
            metrics.misses += u64::from(!hit);
            metrics.hashed_bytes += bytes as u64;
        }
    }
    /// Fixed report ordering keeps native per-frame copies allocation-free.
    #[must_use]
    pub fn values(self) -> [u64; 6] {
        [
            self.hits,
            self.misses,
            self.evictions,
            self.hashed_bytes,
            self.scalar_keys,
            self.preparations,
        ]
    }
}
