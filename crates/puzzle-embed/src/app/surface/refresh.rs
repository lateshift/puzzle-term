//! Host display-link delivery selects an alternative source, never another renderer.
//!
//! The first valid call selects host pacing until this surface closes. A host must
//! keep supplying ticks while its visible surface needs frames. Native subscriptions
//! detach, so two clocks cannot reserve independent submissions for the same surface.
//! Call from the creating host thread, forwarding the display callback's clock facts;
//! no Session operation, GPU acquisition or frame building runs in this call.
use super::*;
use crate::Refresh;
use std::time::Instant;

impl Surface {
    /// Supplies a physical refresh and selects host pacing for this surface until close.
    ///
    /// `display` must match the latest SurfaceInfo. `index` must increase on that
    /// display and survive source restarts; duplicates and old-display ticks are ignored.
    /// Clock values use std::time::Instant: callback <= now, output >= callback,
    /// period > zero. Capture callback on the source thread before forwarding to the
    /// host thread; that delivery delay remains observable without doing frame work here.
    /// Hidden/zero-size surfaces do not present. Input remains independently queued.
    pub fn refresh(&self, refresh: Refresh) -> Result<(), ApiError> {
        self.check()?;
        if refresh.period.is_zero()
            || refresh.callback > Instant::now()
            || refresh.output < refresh.callback
        {
            return Err(ApiError::InvalidArgument("invalid refresh clock"));
        }
        // The actor retains source choice; latest callback coalesces in its mailbox.
        self.update(SurfaceUpdate::HostRefresh(refresh))
    }
}
