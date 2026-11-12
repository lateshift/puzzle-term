//! Xterm XTSHIFTESCAPE changes a program preference, not a user setting.
//!
//! Reference: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html
//! CSI > Ps s uses zero for release and one for capture; omitted means zero.
//! Additional parameters and unknown values are rejected. No grid damage or allocation is necessary:
//! Session publishes the changed input policy independently of frames.
use crate::Terminal;
use vte::Params;

/// Accepts only the single scalar parameter grammar; malformed input does nothing.
pub(super) fn dispatch(term: &mut Terminal, params: &Params) {
    let mut parts = params.iter();
    let value = match parts.next() {
        None => 0,
        Some([value]) => *value,
        // Subparameters are not part of this xterm control function.
        Some(_) => return,
    };
    if parts.next().is_none() && value <= 1 {
        term.mouse_shift_capture = Some(value == 1);
    }
}
