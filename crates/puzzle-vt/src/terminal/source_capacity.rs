//! Shared owner-only source allocations have one transactional byte admission.
//! Private marker pools and precise write watches are reusable VT machinery,
//! not protocol state. The registered consumer supplies the remaining allowance;
//! rejected growth changes no handles, counters, watches or retained capacities.
//!
//! Reserving memory does not advance either monotonic receipt issuer. A later
//! capture can therefore retry after refusal without losing a usable identity.
//! These pools contain only owner-side source references; public selection and
//! frame-publication storage are untouched and remain independently owned.
use super::Terminal;

/// The Terminal is the singular commit point for both private source pools.
impl Terminal {
    /// Reserve upcoming private endpoints and range watches under one byte cap.
    /// `markers` excludes the two gap trackers owned by each new watch. The
    /// maximum includes both existing pools and old/new allocations during growth.
    /// Return false on overflow, allocation refusal or insufficient allowance.
    pub fn reserve_private_source(
        &mut self,
        markers: usize,
        watches: usize,
        maximum: usize,
    ) -> bool {
        let Some(markers) = watches.checked_mul(2).and_then(|n| n.checked_add(markers)) else {
            return false;
        };
        // Each proposal is prepared with its already-retained peer excluded.
        // Thus a known-over-budget request allocates no speculative pool.
        let Some(private_limit) = maximum.checked_sub(self.source_watch_storage_bytes()) else {
            return false;
        };
        let Some((private, private_peak)) = self
            .tracked
            .prepare_private_capacity(markers, private_limit)
        else {
            return false;
        };
        let Some(range_limit) = maximum.checked_sub(private_peak) else {
            return false;
        };
        let Some((ranges, range_peak)) = self
            .write_clock
            .prepare_watch_capacity(watches, range_limit)
        else {
            return false;
        };
        // Every staged Vec exposes its actual capacity. Check both proposals
        // together before either replaces its live owner, never after growth.
        if private_peak
            .checked_add(range_peak)
            .is_none_or(|n| n > maximum)
        {
            return false;
        }
        // No source value or serial was issued during planning. These two
        // infallible moves are the sole commit boundary for the shared pools.
        self.tracked.commit_private_capacity(private);
        self.write_clock.commit_watch_capacity(ranges);
        true
    }
}
