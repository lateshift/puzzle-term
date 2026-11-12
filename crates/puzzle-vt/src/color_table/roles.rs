//! Typed colour-table addresses shared by every protocol and the renderer.
//! Numeric conversions are range checked before constructing a slot (R8).

/// Xterm OSC 5 special colours; themes currently have no separate matching roles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SpecialColor {
    /// Bold text colour (OSC 5 index 0 / OSC 4 index 256).
    Bold,
    /// Underlined text colour (OSC 5 index 1 / OSC 4 index 257).
    Underline,
    /// Blinking text colour (OSC 5 index 2 / OSC 4 index 258).
    Blink,
    /// Reversed text colour (OSC 5 index 3 / OSC 4 index 259).
    Reverse,
    /// Italic text colour (OSC 5 index 4 / OSC 4 index 260).
    Italic,
}

impl SpecialColor {
    /// Rejects unknown special addresses rather than aliasing a valid slot.
    pub fn from_index(index: u16) -> Option<Self> {
        Some(match index {
            0 => Self::Bold,
            1 => Self::Underline,
            2 => Self::Blink,
            3 => Self::Reverse,
            4 => Self::Italic,
            _ => return None,
        })
    }
}

/// Xterm OSC 10–19 dynamic roles followed by kitty's additional roles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum DynamicColor {
    /// OSC 10 default glyph ink.
    Foreground,
    /// OSC 11 default paper and padding.
    Background,
    /// OSC 12 text-cursor fill.
    Cursor,
    /// OSC 13 pointer ink (stored/reported, host pointer is unchanged).
    PointerForeground,
    /// OSC 14 pointer paper (stored/reported only).
    PointerBackground,
    /// OSC 15 Tektronix ink (stored/reported only).
    TektronixForeground,
    /// OSC 16 Tektronix paper (stored/reported only).
    TektronixBackground,
    /// OSC 17 selected-cell paper.
    SelectionBackground,
    /// OSC 18 Tektronix cursor (stored/reported only).
    TektronixCursor,
    /// OSC 19 selected-cell ink.
    SelectionForeground,
    /// OSC 21 cursor_text: glyph under a filled cursor.
    CursorText,
    /// OSC 21 visual_bell: semantic overlay flash colour.
    VisualBell,
}

impl DynamicColor {
    /// Resolves the contiguous xterm numbers without accepting kitty-only roles.
    pub fn from_osc(number: u16) -> Option<Self> {
        Some(match number {
            10 => Self::Foreground,
            11 => Self::Background,
            12 => Self::Cursor,
            13 => Self::PointerForeground,
            14 => Self::PointerBackground,
            15 => Self::TektronixForeground,
            16 => Self::TektronixBackground,
            17 => Self::SelectionBackground,
            18 => Self::TektronixCursor,
            19 => Self::SelectionForeground,
            _ => return None,
        })
    }
}

/// One address in the unified table, independent of the sequence that names it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorSlot {
    /// An ANSI/cube/grey palette entry, always a concrete RGB value.
    Palette(u8),
    /// One xterm special text role.
    Special(SpecialColor),
    /// A theme or dynamic rendering role.
    Dynamic(DynamicColor),
}
