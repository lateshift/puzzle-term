//! Geometry request admission and allocation extents are independent of AppKit.
//!
//! A timed-out transaction still owns an outstanding request. Later transactions
//! may replace its requested size, but cannot wait again until the GPU answers.
//! Allocation buckets grow geometrically; a drag never reallocates per pixel.
use super::Geometry;

/// Latest-wins requests share one monotonic completion namespace per surface.
#[derive(Default)]
pub(super) struct Requests {
    /// Native hierarchy identity changes when the retained layer enters another host.
    binding: Option<usize>,
    /// Last geometry sent, including a request whose bounded wait timed out.
    latest: Option<Geometry>,
    /// Zero denotes no geometry request; actual ordinals never wrap.
    sent: u64,
    /// GPU completion returns credit even when main discards superseded pixels.
    answered: u64,
}
/// Completion credit applies to a request chain, never independently to each size.
impl Requests {
    /// A new host transaction must not inherit an abandoned synchronous wait gate.
    pub(super) fn bind(&mut self, identity: usize) -> bool {
        let moved = self.binding.is_some_and(|old| old != identity);
        self.binding = Some(identity);
        if moved {
            // Keep the sequence monotonic: a delayed old-host completion cannot
            // answer the first request belonging to this new attachment.
            self.answered = self.sent;
            self.latest = None;
        }
        moved
    }
    /// Waiting credit gates only blocking, never delivery of changed geometry.
    pub(super) fn opportunity(
        &mut self,
        committed: Option<Geometry>,
        geometry: Geometry,
        display: bool,
        writable: bool,
    ) -> Option<bool> {
        if committed == Some(geometry) {
            return None;
        }
        // A pending/declined request is still async demand. Reposting it is
        // idempotent: the render mailbox keeps one latest geometry, not a queue.
        let credit = self.request(geometry, writable).unwrap_or(false);
        Some(display && credit)
    }
    /// Returns one new request and whether it may use the sole waiting credit.
    pub(super) fn request(&mut self, geometry: Geometry, writable: bool) -> Option<bool> {
        if self.latest == Some(geometry) && self.sent != self.answered {
            return None;
        }
        // Never park the transaction that must release the compositor-owned pool.
        let may_wait = self.sent == self.answered && writable;
        // Exhaustion disables further synchronous requests rather than wrapping
        // a completion identity; ordinary asynchronously requested frames continue.
        self.sent = self.sent.checked_add(1)?;
        self.latest = Some(geometry);
        Some(may_wait)
    }
    /// A texture captures its request ordinal before GPU submission, never later.
    pub(super) fn ordinal(&self, geometry: Geometry) -> u64 {
        if self.latest == Some(geometry) {
            self.sent
        } else {
            0
        }
    }
    /// An older completion cannot release a newer request's outstanding credit.
    pub(super) fn complete(&mut self, ordinal: u64) {
        self.answered = self.answered.max(ordinal);
    }
}

/// The next geometric bucket is capped by the actual device, with no tuned margin.
pub(super) fn capacity(size: [u32; 2], limit: u32) -> [u32; 2] {
    size.map(|n| n.checked_next_power_of_two().unwrap_or(limit).min(limit))
}
/// Source coordinates select exactly the rendered top-left device-pixel rectangle.
/// contentsRect uses image coordinates; layer flipping affects gravity, not this crop.
pub(super) fn crop(size: [u32; 2], allocation: [u32; 2]) -> objc2_core_foundation::CGRect {
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    CGRect::new(
        CGPoint::new(0., 0.),
        CGSize::new(
            f64::from(size[0]) / f64::from(allocation[0]),
            f64::from(size[1]) / f64::from(allocation[1]),
        ),
    )
}
