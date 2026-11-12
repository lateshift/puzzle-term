//! Pointer hit testing between CSS, device-pixel and cell coordinate spaces.
//!
//! The canvas bounding rectangle already includes the host page's scroll and
//! margins. Its subtraction happens before this helper; multiplying by DPR
//! then maps to the physical backing pixels without a second CSS scale.
//! Grid padding is expressed in that same physical space and removed here.
//!
//! Capture permits events outside the canvas. Cell reports clamp to the visible
//! grid, while SGR-pixel reports keep their nonnegative physical position.
//! The two encodings can therefore describe the same gesture without treating
//! CSS pixels as cells or applying DPR twice.
//!
//! DOM buttons and terminal buttons agree on left/middle/right numbers for a
//! press, but buttons is a different bitmask: middle is bit 4, right is bit 2.
//! A motion must recover the button from that mask because event.button is -1.
//! The held flag also controls whether mode 1002 requests that movement.
use crate::dom::{Geometry, keyboard};
use puzzle_term::vt::input::{MouseEvent, MouseKind};
use web_sys::MouseEvent as DomMouse;

/// Maps a pointer snapshot independently of DOM registration for table tests.
pub(crate) fn map(event: &DomMouse, geometry: Geometry, position: [f64; 2]) -> MouseEvent {
    let pixels = [
        position[0] - geometry.padding[0],
        position[1] - geometry.padding[1],
    ];
    // Rust float-to-integer casts saturate: negative/outside events cannot wrap.
    let column = (pixels[0] / geometry.cell[0]) as u32;
    let row = (pixels[1] / geometry.cell[1]) as u32;
    MouseEvent {
        kind: match event.type_().as_str() {
            "pointerup" | "pointercancel" => MouseKind::Release,
            "pointermove" => MouseKind::Move,
            _ => MouseKind::Press,
        },
        button: if event.type_() == "pointermove" {
            if event.buttons() & 1 != 0 {
                0
            } else if event.buttons() & 4 != 0 {
                1
            } else {
                2
            }
        } else {
            event.button().max(0) as u16
        },
        column: column.min(geometry.grid[0].saturating_sub(1)),
        row: row.min(geometry.grid[1].saturating_sub(1)),
        pixel_x: pixels[0] as u32,
        pixel_y: pixels[1] as u32,
        modifiers: keyboard::modifiers(
            event.shift_key(),
            event.alt_key(),
            event.ctrl_key(),
            event.meta_key(),
        ),
        button_held: event.buttons() != 0,
        click_count: event.detail().clamp(0, 255) as u8,
    }
}
