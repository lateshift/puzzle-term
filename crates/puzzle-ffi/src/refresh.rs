//! Relative host timestamps translate into the safe embedding refresh value.
//!
//! All values are copied; no native callback pointer or borrowed clock escapes.
//! Output delay is measured from this call, so hosts forwarding a source callback
//! subtract their queueing delay first. Duplicate identities remain harmless.
use crate::{abi::ResultCode, registry};
use std::time::{Duration, Instant};

/// Selects host pacing until surface close and queues one physical refresh.
///
/// Output delay and period are nanoseconds on a monotonic clock. The creating
/// thread forwards callbacks here; rendering stays on Puzzle's worker. Zero period
/// or an unrepresentable future output instant is rejected before source selection.
#[allow(
    unsafe_code,
    reason = "D-43: C refresh export contains copied clock facts only"
)]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_refresh(
    id: u64,
    display: u64,
    index: u64,
    output_delay_ns: u64,
    period_ns: u64,
) -> ResultCode {
    // Relative values avoid assuming the host and Rust clocks share an epoch.
    let callback = Instant::now();
    let Some(output) = callback.checked_add(Duration::from_nanos(output_delay_ns)) else {
        return ResultCode::InvalidArgument;
    };
    // Capability/affinity validation happens on the same safe Surface method as Rust hosts.
    registry::with_surface(id, |surface| {
        surface.refresh(puzzle_embed::Refresh {
            display,
            index,
            output,
            callback,
            period: Duration::from_nanos(period_ns),
        })
    })
}
