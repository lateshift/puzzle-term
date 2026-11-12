//! Small frozen command intervals produced by the Session's OSC mark owner.
//!
//! The render half receives only visible line extents, status and duration;
//! output extraction remains on the owner and occurs only after a copy gesture.
use puzzle_vt::tracked::LineId;
/// One visible slice of a shell command, retaining its identity across reflow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    /// Untrusted OSC cannot choose or reuse this VT-assigned identity.
    pub id: u64,
    /// First visible row touched by the output interval.
    pub first: LineId,
    /// Last visible row touched by the output interval.
    pub last: LineId,
    /// Missing while running or when the shell omits its completion status.
    pub exit: Option<i32>,
    /// Complete elapsed seconds, excluding the prompt and input-editing phase.
    pub duration: Option<f64>,
}
