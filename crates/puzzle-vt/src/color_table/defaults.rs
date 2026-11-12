//! Xterm palette and historical unconfigured harness role defaults.
//! These defaults preserve existing headless reference pixels. Configured hosts
//! install authored theme tokens before the first presentation. The VT still
//! needs deterministic defaults for colour queries before any host is attached.
//! A dynamic value means the renderer derives ink from the cell, not black.
//! Numeric role order is defined by DynamicColor, so each array entry names
//! its corresponding role below. All entries begin without an OSC override.
use super::{ColorValue, Entry};

/// Preserves original default, cursor and selection pixels until theme installation.
pub(super) fn dynamic() -> [Entry; 12] {
    use ColorValue::{Dynamic, Rgb};
    // Pointer/Tektronix defaults follow ordinary ink/paper; they are reported only.
    [
        Rgb([229; 3]),     // Foreground: historical unconfigured light ink.
        Rgb([14, 18, 26]), // Background also supplies surface padding.
        Rgb([229; 3]),     // Cursor fill follows unconfigured foreground.
        Rgb([229; 3]),     // Pointer foreground is queryable without an OS cursor.
        Rgb([14, 18, 26]), // Pointer background pairs with pointer ink.
        Rgb([229; 3]),     // Tektronix foreground is retained, not emulated.
        Rgb([14, 18, 26]), // Tektronix background likewise has no screen.
        Rgb([53, 70, 93]), // Selection background matches existing M8 references.
        Rgb([229; 3]),     // Tektronix cursor uses the ordinary cursor default.
        Dynamic,           // Absent selection ink preserves the selected cell's ink.
        Rgb([14, 18, 26]), // Cursor text remains readable against its light fill.
        Dynamic,           // Unconfigured visual bell has no fixed colour report.
    ]
    .map(Entry::new)
}

/// Provides the fixed xterm 256-color table where no OSC override exists.
pub fn palette(index: u8) -> [u8; 3] {
    // Original xterm ANSI defaults, replaced by a resolved host theme.
    const ANSI: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 0, 0],
        [0, 205, 0],
        [205, 205, 0],
        [0, 0, 238],
        [205, 0, 205],
        [0, 205, 205],
        [229, 229, 229],
        [127, 127, 127],
        [255, 0, 0],
        [0, 255, 0],
        [255, 255, 0],
        [92, 92, 255],
        [255, 0, 255],
        [0, 255, 255],
        [255, 255, 255],
    ];
    match index {
        // Host themes replace these initial defaults when installed.
        0..=15 => ANSI[index as usize],
        16..=231 => {
            // The cube has six levels per axis: 0, 95, 135, …, 255.
            let offset = index - 16;
            let level = |n: u8| if n == 0 { 0 } else { n * 40 + 55 };
            [
                level(offset / 36),
                level((offset / 6) % 6),
                level(offset % 6),
            ]
        }
        _ => {
            // Neutral ramp has 24 entries, spaced by ten starting at eight.
            let grey = 8 + (index - 232) * 10;
            [grey, grey, grey]
        }
    }
}
