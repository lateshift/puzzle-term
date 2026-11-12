//! Bounded ANSI and DEC mode registry for xterm's DECRQM (§6.5).
//!
//! Each supported mode has a fixed bit. Unknown numbers are ignored on
//! DECSET/DECRST and reported as zero by DECRQM; hostile numeric values
//! therefore cannot grow a map or create an accidental supported mode.

/// Compact state of all advertised terminal modes.
#[derive(Clone, Copy, Debug)]
pub struct Modes {
    /// One bit per supported ANSI or DEC private mode.
    bits: u64,
}

impl Default for Modes {
    /// Enables only the xterm defaults that affect visible behaviour.
    fn default() -> Self {
        let mut modes = Self { bits: 0 };
        modes.set(true, 7, true);
        modes.set(true, 12, true);
        modes.set(true, 25, true);
        // §7.4 enables wheel-to-arrow conversion by default on alternate screens.
        modes.set(true, 1007, true);
        modes.set(true, 1036, true);
        modes.set(true, 1039, true);
        // D-31 follows the reference terminal's clustered default; applications can still
        // request scalar widths with DEC private mode 2027 reset.
        modes.set(true, 2027, true);
        modes
    }
}

impl Modes {
    /// Changes a listed mode, ignoring unknown numbers without allocation.
    pub fn set(&mut self, private: bool, number: u16, enabled: bool) {
        let Some(bit) = mode_bit(private, number) else {
            return;
        };
        if enabled {
            self.bits |= bit;
        } else {
            self.bits &= !bit;
        }
    }

    /// Returns `None` for modes the terminal does not advertise.
    #[must_use]
    pub fn get(&self, private: bool, number: u16) -> Option<bool> {
        mode_bit(private, number).map(|bit| self.bits & bit != 0)
    }

    /// Resets the mode table to terminal defaults after RIS or DECSTR.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Maps documented DEC mode numbers onto dense slots for bounded storage.
///
/// The order is an implementation detail; the public numbers are the xterm
/// protocol. Grouping the table by purpose makes omissions visible when
/// comparing it to design §6.5 and xterm ctlseqs.
pub(crate) const DEC_MODES: &[(u16, u8)] = &[
    // Keyboard, column and global-video modes precede cursor controls.
    (1, 2),
    // DECCOLM is reported despite host-controlled width in Puzzle.
    (3, 3),
    // Reverse video belongs to rendering, not per-cell color mutation.
    (5, 4),
    // DECOM changes the reference point for CUP coordinates.
    (6, 5),
    // DECAWM is on by default and drives delayed wrap after the last cell.
    (7, 6),
    // Blink and visibility are separate cursor presentation switches.
    (12, 7),
    (25, 8),
    // ?40 allows DECCOLM's clear without changing the host window size.
    (40, 9),
    // Reverse wrap is valid only across an explicitly soft-wrapped row.
    (45, 10),
    (47, 11),
    (66, 12),
    // DECLRMM turns CSI s into DECSLRM instead of cursor save.
    (69, 13),
    // Extended alternate-screen modes have distinct save/clear semantics.
    (1047, 14),
    (1048, 15),
    (1049, 16),
    // Mouse tracking and encoding modes are separately reportable.
    (9, 17),
    (1000, 18),
    (1002, 19),
    (1003, 20),
    (1004, 21),
    // UTF-8, SGR, urxvt and pixel reports are distinct wire encodings.
    (1005, 22),
    (1006, 23),
    (1015, 24),
    (1016, 25),
    (1007, 26),
    // Meta/Alt ESC behaviour is an input-encoder concern in M5.6.
    (1036, 27),
    (1039, 28),
    // Bracketed paste changes the pure input encoder in M5.6.
    (2004, 29),
    // New extensions still answer DECRQM before all host wiring exists.
    (2026, 30),
    (2027, 31),
    // Appearance reports and in-band resize are opt-in host-facing modes.
    (2031, 32),
    (2048, 33),
    // MIME paste-event mode, defined at rockorager.dev/misc/bracketed-paste-mime/.
    (5522, 34),
];

/// Assigns stable bit positions independently of the large DEC mode number.
fn mode_bit(private: bool, number: u16) -> Option<u64> {
    let index = if private {
        // This linear scan runs only on control sequences, never per cell.
        DEC_MODES.iter().find(|entry| entry.0 == number)?.1
    } else {
        // ANSI IRM and LNM use their own namespace, without a `?` prefix.
        match number {
            4 => 0,
            20 => 1,
            _ => return None,
        }
    };
    Some(1_u64 << index)
}

#[cfg(test)]
#[path = "modes/tests.rs"]
mod tests;
