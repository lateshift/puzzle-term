//! Shared cold metrics keep runtime input clocks independent of feed instrumentation.
//!
//! Every embedder can enable the private input trace without rebuilding. The
//! larger feed/pool journals and their timers retain their previous debug or
//! lock-metrics compile gate; ordinary optimized parsing does no observer work.
/// Paired API, control, owner and ordered-writer input observations.
mod control;
/// Full parser/pool metrics never enter an ordinary optimized host build.
#[cfg(any(debug_assertions, feature = "lock-metrics"))]
mod feed;
/// Fixed worst-five records are also reused by optional feed observers.
mod hold;
#[cfg(any(debug_assertions, feature = "lock-metrics"))]
pub use feed::Stage;

/// One surface owns fixed counters, shared only by its platform endpoints.
#[derive(Default)]
pub struct Metrics {
    /// Input observation can be activated by the private tracing target.
    pub control: control::Controls,
    /// Parsing storage and timers remain opt-in in optimized builds.
    #[cfg(any(debug_assertions, feature = "lock-metrics"))]
    feed: feed::Feed,
}

/// Existing feed call sites project directly into the compile-time observer.
#[cfg(any(debug_assertions, feature = "lock-metrics"))]
impl std::ops::Deref for Metrics {
    /// The projection adds neither a lock nor a conditional branch to feed probes.
    type Target = feed::Feed;
    /// Only the platform owner records these counters; they contain no VT borrow.
    fn deref(&self) -> &Self::Target {
        &self.feed
    }
}

/// Formatting occurs after a finite instrumented cohort, never inside a key.
#[cfg(feature = "lock-metrics")]
impl Metrics {
    /// Retains the original combined feed/input JSON receipt for older gates.
    pub fn report(&self) -> serde_json::Value {
        self.feed.report(self.control.report())
    }
}
