//! DEC and xterm mouse reports from neutral pointer events (§6.8).
//!
//! Coordinates supplied here are zero-based; every wire form adds one.
//! SGR-pixel mode uses device-pixel coordinates, while all other modes
//! use cell coordinates. Tracking filters motion before formatting bytes,
//! so a disabled report never allocates an output buffer.
//!
//! Tracking mode controls *which* transitions may leave the terminal;
//! encoding mode controls only their byte form. Keeping those decisions
//! separate avoids a release report in press-only X10 mode and preserves
//! local Shift selection even when an application tracks every movement.
use super::key::Modifiers;

/// Which pointer events the application requested with DECSET.
///
/// Mode 1003 is the only one that reports motion with no held button.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MouseTracking {
    /// No reporting; the host can select text locally.
    #[default]
    None,
    /// DECSET 9 reports button presses only.
    X10,
    /// DECSET 1000 adds button releases.
    Normal,
    /// DECSET 1002 adds movement while a button is down.
    ButtonMotion,
    /// DECSET 1003 reports all movement.
    AnyMotion,
}

/// Wire coordinate and byte format chosen by DECSET 1005/6/15/16.
///
/// Only SGR-pixel mode changes the coordinate unit; SGR itself is cells.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MouseEncoding {
    /// Original X10 bytes, bounded to 223 one-based cells.
    #[default]
    Default,
    /// Xterm 1005 UTF-8 encoded coordinate values.
    Utf8,
    /// Xterm 1006 decimal SGR form.
    Sgr,
    /// Urxvt 1015 decimal form with the byte offset retained.
    Urxvt,
    /// Xterm 1016 SGR form, with device-pixel positions.
    SgrPixels,
}

/// Input transition carried by one pointer event.
///
/// Wheel directions are button codes rather than a separate wire final.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseKind {
    /// A button from the §6.8 numbering was pressed.
    Press,
    /// A button was released.
    Release,
    /// Pointer movement, possibly with a held button.
    Move,
    /// Wheel direction encoded as button 64–67.
    Wheel,
}

/// A pointer position and action before the host's report policy.
///
/// Adapters compute cell and device-pixel coordinates from the same
/// physical event; this encoder chooses whichever the mode requires.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MouseEvent {
    /// Pressed/released/moved/wheeled transition.
    pub kind: MouseKind,
    /// §6.8 button number: left 0, middle 1, right 2, wheels 64–67.
    pub button: u16,
    /// Zero-based cell column.
    pub column: u32,
    /// Zero-based cell row.
    pub row: u32,
    /// Zero-based device-pixel x position for mode 1016.
    pub pixel_x: u32,
    /// Zero-based device-pixel y position for mode 1016.
    pub pixel_y: u32,
    /// Shift, Alt and Control flags of the pointer gesture.
    pub modifiers: Modifiers,
    /// Whether any pointer button is held during a motion event.
    pub button_held: bool,
    /// Consecutive presses at one cell, reserved for local M8 selection.
    pub click_count: u8,
}

/// Encodes a report or returns no bytes when tracking/policy suppresses it.
#[must_use]
pub fn encode_mouse(
    event: MouseEvent,
    tracking: MouseTracking,
    encoding: MouseEncoding,
    shift_selects: bool,
) -> Vec<u8> {
    // Shift lets users select even in applications that captured the mouse.
    if tracking == MouseTracking::None || (shift_selects && event.modifiers.has(Modifiers::SHIFT)) {
        return Vec::new();
    }
    if event.kind == MouseKind::Move
        && (tracking == MouseTracking::X10
            || tracking == MouseTracking::Normal
            || (tracking == MouseTracking::ButtonMotion && !event.button_held))
    {
        return Vec::new();
    }
    if event.kind == MouseKind::Release && tracking == MouseTracking::X10 {
        return Vec::new();
    }
    let release = event.kind == MouseKind::Release;
    // Legacy wire formats report button 3 for release; SGR keeps identity
    // and changes the final to lowercase m instead (xterm ctlseqs).
    let mut button =
        if release && !matches!(encoding, MouseEncoding::Sgr | MouseEncoding::SgrPixels) {
            3
        } else {
            event.button
        };
    button = button.saturating_add(
        u16::from(event.modifiers.0 & (Modifiers::SHIFT | Modifiers::ALT | Modifiers::CTRL)) * 4,
    );
    if event.kind == MouseKind::Move {
        button = button.saturating_add(32);
    }
    let (x, y) = if encoding == MouseEncoding::SgrPixels {
        (
            event.pixel_x.saturating_add(1),
            event.pixel_y.saturating_add(1),
        )
    } else {
        (event.column.saturating_add(1), event.row.saturating_add(1))
    };
    match encoding {
        MouseEncoding::Sgr | MouseEncoding::SgrPixels => {
            format!("\x1b[<{button};{x};{y}{}", if release { 'm' } else { 'M' }).into_bytes()
        }
        MouseEncoding::Urxvt => crate::protocols::rxvt::mouse_report(button, x, y),
        MouseEncoding::Default | MouseEncoding::Utf8 => legacy_bytes(button, x, y, encoding),
    }
}

/// Applies the old 32-byte offset, rejecting coordinates it cannot encode.
fn legacy_bytes(button: u16, x: u32, y: u32, encoding: MouseEncoding) -> Vec<u8> {
    let values = [
        u32::from(button) + 32,
        x.saturating_add(32),
        y.saturating_add(32),
    ];
    if encoding == MouseEncoding::Default && values.iter().any(|value| *value > 255) {
        return Vec::new();
    }
    let mut bytes = b"\x1b[M".to_vec();
    for value in values {
        if let Some(ch) = char::from_u32(value) {
            if encoding == MouseEncoding::Utf8 {
                let mut encoded = [0; 4];
                bytes.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
            } else {
                bytes.push(value as u8);
            }
        } else {
            return Vec::new();
        }
    }
    bytes
}
