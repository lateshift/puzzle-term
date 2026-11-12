//! Resolves pointer reporting modes from the active terminal (§6.8).
//!
//! Tracking and encoding are independent. Higher modern modes win when an
//! application leaves several enabled, as xterm ctlseqs specifies. Hosts
//! read this same snapshot for coalescing; Session rechecks it when writing.
use super::{MouseEncoding, MouseTracking};

/// Small copied mode snapshot, with no platform state or clock.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MouseModes {
    /// Which transitions may be reported.
    pub tracking: MouseTracking,
    /// Coordinate units and historical byte format.
    pub encoding: MouseEncoding,
}

/// Resolves the terminal's stored DEC modes without changing them.
impl MouseModes {
    /// Takes the highest enabled tracking and encoding mode.
    pub fn from_terminal(term: &crate::Terminal) -> Self {
        let enabled = |number| term.modes.get(true, number) == Some(true);
        // Prefer all-motion over button-motion, normal and press-only.
        let tracking = [
            (1003, MouseTracking::AnyMotion),
            (1002, MouseTracking::ButtonMotion),
            (1000, MouseTracking::Normal),
            (9, MouseTracking::X10),
        ]
        .into_iter()
        .find_map(|(mode, value)| enabled(mode).then_some(value))
        .unwrap_or_default();
        // Pixel SGR is the most expressive; classic bytes remain the default.
        let encoding = [
            (1016, MouseEncoding::SgrPixels),
            (1006, MouseEncoding::Sgr),
            (1015, MouseEncoding::Urxvt),
            (1005, MouseEncoding::Utf8),
        ]
        .into_iter()
        .find_map(|(mode, value)| enabled(mode).then_some(value))
        .unwrap_or_default();
        Self { tracking, encoding }
    }
}
