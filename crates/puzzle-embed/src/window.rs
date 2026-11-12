//! Native surface execution and refresh policy, independent of any window toolkit.
/// One surface's scheduling runs only on the process rendering actor.
pub mod driver;
/// Optional scalar phase observations shared with developer reports.
#[cfg(feature = "lock-metrics")]
pub mod frame_work;
/// Persistent sealed-snapshot and GPU surface owner.
pub mod live;
/// Diagnostic hosts attach measurements without defining a second frame loop.
#[cfg(feature = "lock-metrics")]
pub mod observer;
/// D-33 timing and D-53 display callbacks share this tested clock policy.
pub mod pacing;
/// Event and presentation counters contain no terminal ownership.
pub mod perf;
