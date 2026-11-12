//! Standard hosts need no additional process lifecycle state.
use crate::app::{ApiError, SurfaceOptions, pty_bridge};

/// Zero-sized cold policy for a shell or host-owned byte stream.
#[derive(Default)]
pub(in crate::app) struct State {
    /// Reserve a zero-sized slot for future cold policy without affecting the ABI.
    _reserved: (),
}
/// The cold interface matches the optional implementation, so callers need no cfg branches.
impl State {
    /// Ordinary sessions have no additional launch grant.
    pub(in crate::app) fn provision(
        &mut self,
        _: &SurfaceOptions,
    ) -> Result<pty_bridge::Launch, ApiError> {
        // None carries the same no-launch state through the common startup transaction.
        Ok(None)
    }
}
