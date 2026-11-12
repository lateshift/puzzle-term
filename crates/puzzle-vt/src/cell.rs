//! Compact cell encoding for a terminal's full-width grid (§6.2).
//!
//! Colors stay tagged rather than resolved to RGB so theme and palette
//! changes recolor existing cells without rewriting the screen. The fixed
//! layout gives each row a predictable byte cost for the scrollback budget.
use bitflags::bitflags;

/// A tagged foreground or background value.
///
/// The top byte selects default (0), palette index (1) or RGB (2).
/// The low 24 bits hold that tag's payload. Rendering resolves the value.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(transparent)]
pub struct PackedColor(pub u32);

impl PackedColor {
    /// The default color follows the currently selected theme.
    pub const DEFAULT: Self = Self(0);

    /// Stores an index without resolving the mutable 256-color palette.
    #[must_use]
    pub const fn palette(index: u8) -> Self {
        Self((1 << 24) | index as u32)
    }

    /// Stores a true-color value in the low three bytes.
    #[must_use]
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self((2 << 24) | ((red as u32) << 16) | ((green as u32) << 8) | blue as u32)
    }
}

bitflags! {
    /// Inline cell attributes; underline style occupies bits 8–10.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub struct Attrs: u16 {
        /// Increased foreground weight.
        const BOLD = 1 << 0;
        /// Reduced foreground intensity.
        const DIM = 1 << 1;
        /// Italic or oblique face request.
        const ITALIC = 1 << 2;
        /// Blink request, interpreted by the renderer.
        const BLINK = 1 << 3;
        /// Exchanges foreground and background at rendering.
        const INVERSE = 1 << 4;
        /// Hides ink while retaining selectable text.
        const HIDDEN = 1 << 5;
        /// Draws a line through the glyph.
        const STRIKE = 1 << 6;
        /// Draws a line above the glyph.
        const OVERLINE = 1 << 7;
        /// Reserves three bits for none/single/double/curly/dotted/dashed.
        const UNDERLINE_MASK = 0b111 << 8;
    }
}

impl Attrs {
    /// Returns the SGR underline style without changing the other flags.
    #[must_use]
    pub const fn underline_style(self) -> u8 {
        ((self.bits() >> 8) & 0b111) as u8
    }

    /// Replaces only the underline style's three bits.
    pub fn set_underline_style(&mut self, style: u8) {
        // The mask reserves the range, and SGR styles fit in 0..=5.
        let bits = (self.bits() & !Self::UNDERLINE_MASK.bits()) | (u16::from(style & 7) << 8);
        *self = Self::from_bits_retain(bits);
    }
}

bitflags! {
    /// Layout flags associated with one screen cell.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub struct CellFlags: u8 {
        /// First column of a two-column character.
        const WIDE = 1 << 0;
        /// Placeholder in its second column, never shaped as text.
        const WIDE_SPACER = 1 << 1;
        /// A sparse row extra contains cluster/link/underline data.
        const HAS_EXTRAS = 1 << 2;
        /// Selective erase skips this cell (DECSCA).
        const PROTECTED = 1 << 3;
        /// ch is a screen-local OSC 66 slot index, never a Unicode scalar.
        const MULTICELL = 1 << 4;
    }
}

/// One grid column, including inline style, at exactly 16 bytes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct Cell {
    /// Unicode scalar, zero for unwritten blank or `u32::MAX` for a cluster.
    pub ch: u32,
    /// Tagged foreground, resolved only at render time.
    pub fg: PackedColor,
    /// Tagged background, retained across palette changes.
    pub bg: PackedColor,
    /// Attributes including a three-bit underline style.
    pub attrs: Attrs,
    /// Width, extra-data and protection markers.
    pub flags: CellFlags,
    /// Explicit padding keeps the layout stable across targets.
    reserved: u8,
}

impl Cell {
    /// Kitty graphics placeholder: a one-cell private scalar, never font ink.
    pub const IMAGE_PLACEHOLDER: u32 = 0x10eeee;

    /// Sentinel identifying a row-extra grapheme cluster.
    pub const CLUSTER: u32 = u32::MAX;

    /// Returns a blank using the pen's background-color-erase color (§6.2).
    #[must_use]
    pub const fn erased(background: PackedColor) -> Self {
        Self {
            ch: 0,
            fg: PackedColor::DEFAULT,
            bg: background,
            attrs: Attrs::empty(),
            flags: CellFlags::empty(),
            reserved: 0,
        }
    }
}

#[cfg(test)]
#[path = "cell/tests.rs"]
mod tests;
