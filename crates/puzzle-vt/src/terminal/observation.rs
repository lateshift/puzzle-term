//! Lazy source ordering leaves ordinary unregistered output on row-local stamps.
//! A source consumer activates once, before sampling any witness. The cold scan
//! includes retained history, inactive screen and reusable spare rows, without
//! changing their cache keys. No scan, allocation or clock store occurs in the
//! ordinary scalar path. The last owner/watch release restores that path.
use super::Terminal;

impl Terminal {
    /// Registers a source consumer for its explicit lifetime (including selection).
    /// Call before sampling the clock; balance with unregister when it is retired.
    pub fn register_source_observer(&mut self) {
        self.activate_write_clock();
        self.write_clock.observe();
    }

    /// Retires a registered consumer; remaining owners/watches retain global order.
    pub fn unregister_source_observer(&mut self) {
        self.write_clock.unobserve();
    }

    /// A first watch also needs ordering, even without a separately registered owner.
    pub(super) fn activate_write_clock(&mut self) {
        if !self.write_clock.active() {
            // Local versions can diverge across rows while inactive. Neither the
            // visible screen alone nor the old paused clock bounds all of them.
            let maximum = self
                .primary
                .ring
                .maximum_write_version()
                .max(self.alternate.ring.maximum_write_version());
            self.write_clock.activate(maximum);
        }
    }
}

#[cfg(test)]
/// Activation, reactivation and row-cache/witness continuity are functional tests.
mod tests;
