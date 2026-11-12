//! Host clock pairs separate OS/event-loop residence from Puzzle work (D-61).
//!
//! Hosts can have arbitrary monotonic clock epochs. Subtracting two readings
//! from that same clock avoids guessing how NSEvent, QPC or Rust Instant align.
//! Capture at the API boundary, before input conversion, to include marshalling.
use std::time::{Duration, Instant};

/// Optional host-event observation carried through the ordered input lane.
/// Create immediately before calling a timed input method, before converting
/// native key/text values. Neither timestamp nor input content is logged unless
/// `puzzle_desktop::input=debug` is enabled; ordinary methods remain supported.
#[derive(Clone, Copy, Debug)]
pub struct InputTiming {
    /// Start of the measured API work, including caller-side input conversion.
    pub(crate) api: Instant,
    /// Host clock difference, absent when the native toolkit has no timestamp.
    pub(crate) residence: Option<Duration>,
    /// The endpoint replaces this immediately before the control-channel send.
    pub(crate) sent: Instant,
    /// Captured on owner receipt, before translation or current input-mode routing.
    pub(crate) owner: Option<(Instant, Option<Duration>)>,
    /// Opaque surface identity lets multi-pane embedders distinguish their keys.
    pub(crate) surface: Option<u64>,
}

impl InputTiming {
    /// Capture an API boundary without an OS timestamp, only when tracing is on.
    /// C adapters call before decoding buffers so marshalling remains API work.
    pub fn capture_api() -> Option<Self> {
        Self::capture()
    }

    /// Capture an OS event and a current reading in seconds of the same clock.
    /// Negative, nonfinite or reversed values are rejected, never clamped into
    /// an apparently fast sample. Epoch equality across processes is unnecessary.
    pub fn new(event_seconds: f64, clock_seconds: f64) -> Result<Self, crate::ApiError> {
        let api = Instant::now();
        if !event_seconds.is_finite() || event_seconds < 0.0 || clock_seconds < event_seconds {
            return Err(crate::ApiError::InvalidArgument("invalid input clock pair"));
        }
        let residence = Duration::try_from_secs_f64(clock_seconds - event_seconds)
            .map_err(|_| crate::ApiError::InvalidArgument("invalid input clock pair"))?;
        Ok(Self {
            api,
            residence: Some(residence),
            sent: api,
            owner: None,
            surface: None,
        })
    }

    /// Toolkits without OS timestamps still measure API-to-writer delivery.
    pub(crate) fn capture() -> Option<Self> {
        // No per-input clock reads when the privacy-sensitive target is disabled.
        tracing::enabled!(target: "puzzle_desktop::input", tracing::Level::DEBUG).then(|| {
            let api = Instant::now();
            Self {
                api,
                residence: None,
                sent: api,
                owner: None,
                surface: None,
            }
        })
    }

    /// A diagnostic posted before UI delivery uses its actual monotonic arrival.
    #[cfg(all(feature = "test-support", feature = "lock-metrics"))]
    pub(crate) fn arrived(arrival: Instant) -> Option<Self> {
        Self::capture().map(|mut stamp| {
            stamp.residence = Some(stamp.api.saturating_duration_since(arrival));
            stamp
        })
    }

    /// Preserve pre-translation owner work rather than starting after the adapter.
    pub(crate) fn applied(&mut self) {
        self.owner = Some((Instant::now(), crate::thread_cpu::diagnostic_now()));
    }
}

/// Clock validation never uses scheduler-sensitive timing assertions.
#[cfg(test)]
mod tests;
