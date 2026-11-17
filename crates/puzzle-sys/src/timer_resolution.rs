//! Process timer resolution for render-actor deadline waits.
//!
//! Windows rounds `recv_timeout`/`sleep` wakeups up to the system timer tick,
//! 15.6 ms by default, which overshoots every frame deadline by up to one tick.
//! A held [`Resolution`] requests a 1 ms tick; dropping it releases the request.
//! Other platforms already wake with sub-millisecond precision, so it is inert.

/// Holds a 1 ms system timer resolution request for its lifetime.
#[derive(Debug)]
#[must_use = "the resolution request is released when the guard drops"]
pub struct Resolution {
    /// Only a successful request may be released; Windows pairs begin/end calls.
    #[cfg(windows)]
    active: bool,
}

/// The tick requested while a render actor may wait on frame deadlines.
#[cfg(windows)]
const PERIOD_MS: u32 = 1;

impl Resolution {
    /// Requests 1 ms timer resolution; failure leaves the default tick in place.
    pub fn precise() -> Self {
        #[cfg(windows)]
        {
            #[allow(
                unsafe_code,
                reason = "R8: audited winmm timer glue is confined to puzzle-sys"
            )]
            // SAFETY: timeBeginPeriod takes a plain integer and has no memory effects.
            let status = unsafe { windows_sys::Win32::Media::timeBeginPeriod(PERIOD_MS) };
            let active = status == windows_sys::Win32::Media::TIMERR_NOERROR;
            if active {
                tracing::debug!(
                    event = "sys.timer_resolution",
                    period_ms = PERIOD_MS,
                    "1 ms timer resolution requested"
                );
            } else {
                tracing::warn!(
                    event = "sys.timer_resolution_failed",
                    status,
                    "timer resolution request rejected"
                );
            }
            Self { active }
        }
        #[cfg(not(windows))]
        {
            Self {}
        }
    }
}

impl Drop for Resolution {
    /// Releases exactly the request this guard made.
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.active {
            #[allow(
                unsafe_code,
                reason = "R8: audited winmm timer glue is confined to puzzle-sys"
            )]
            // SAFETY: pairs the successful timeBeginPeriod above with the same period.
            unsafe {
                windows_sys::Win32::Media::timeEndPeriod(PERIOD_MS);
            }
        }
    }
}

#[cfg(test)]
/// A guard can be created and released repeatedly without leaking requests.
mod tests {
    use super::Resolution;

    /// Nested guards pair their own begin/end calls.
    #[test]
    fn guards_nest_and_release() {
        let outer = Resolution::precise();
        let inner = Resolution::precise();
        drop(inner);
        drop(outer);
    }
}
