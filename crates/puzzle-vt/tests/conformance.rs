//! Public-feed conformance cases against xterm ctlseqs (design §6.5).
//!
//! Each family is separate so later protocol additions cannot hide an
//! omitted control behind one broad, synthetic smoke assertion.
#![allow(
    clippy::expect_used,
    reason = "Conformance tests name the missing visible row in a failed fixture"
)]

/// Absolute, relative and origin-mode cursor placement.
#[path = "conformance/cursor.rs"]
mod cursor;
/// DCS status and termcap request replies.
#[path = "conformance/dcs.rs"]
mod dcs;
/// Erase and horizontal editing with background-color erase.
#[path = "conformance/editing.rs"]
mod editing;
/// ESC save/restore, tab, reverse index and reset behaviour.
#[path = "conformance/esc.rs"]
mod esc;
/// Hostile oversized strings and §16 state ceilings.
#[path = "conformance/limits.rs"]
mod limits;
/// DECRQM replies for advertised ANSI and private modes.
#[path = "conformance/modes.rs"]
mod modes;
/// OSC title, palette, links, clipboard and shell marks.
#[path = "conformance/osc.rs"]
mod osc;
/// Resize/reflow behavior in both primary and alternate buffers.
#[path = "conformance/reflow.rs"]
mod reflow;
/// Exact CSI status and identity wire bytes.
#[path = "conformance/replies.rs"]
mod replies;
/// DEC scroll regions, history and alternate-screen identity.
#[path = "conformance/scroll.rs"]
mod scroll;
/// SGR attributes and semicolon/colon color forms.
#[path = "conformance/sgr.rs"]
mod sgr;
/// Unicode width model and opt-in grapheme clustering.
#[path = "conformance/unicode.rs"]
mod unicode;

/// Reads one visible row as plain scalars for concise protocol assertions.
fn row(term: &puzzle_vt::Terminal, index: usize) -> String {
    check_grid(term);
    term.screen()
        .row(index)
        .expect("visible row")
        .cells
        .iter()
        .map(|cell| {
            if cell.ch == 0 {
                ' '
            } else {
                char::from_u32(cell.ch).unwrap_or(' ')
            }
        })
        .collect()
}

/// Conformance helpers validate both screens without modifying the product feed path.
fn check_grid(term: &puzzle_vt::Terminal) {
    #[cfg(debug_assertions)]
    for screen in [&term.primary, &term.alternate] {
        assert_eq!(screen.ring.check_grid(), Ok(()));
    }
    #[cfg(not(debug_assertions))]
    let _ = term;
}

/// Unified colour-table conformance, separated by OSC number.
#[path = "conformance/colors.rs"]
mod colors;

/// Shared Shift ownership negotiated independently of tracking modes.
#[path = "conformance/shift_capture.rs"]
mod shift_capture;

/// UAPI context lifetime is independent of terminal resets.
#[path = "conformance/context.rs"]
mod context;

/// Rectangular text uses kitty metadata and remains independent of fonts.
#[path = "conformance/text_sizing.rs"]
mod text_sizing;
