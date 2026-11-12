//! Shares diagnostic border and positioned hexadecimal digits across all missing scalars.
//!
//! A Unicode flood can contain tens of thousands of absent codepoints. Preparing
//! their combined outlines separately would exhaust the unchanged Slug budget.
//! The mini alphabet has fixed positions, so at most 129 component shapes describe
//! every u32 label at one cell geometry, without allocating during enumeration.
use super::{BuiltinCell, BuiltinInk, PixelPen, symbols};
use kurbo::BezPath;

/// Splits diagnostic geometry without changing the original codepoint label.
impl BuiltinCell {
    /// Enumerates border zero followed by positioned digit identities, without leading zeroes.
    pub fn tofu_components(self, code: u32) -> impl Iterator<Item = u32> {
        // Each nibble occupies four bits; zero still has one printable digit.
        let digits = (32 - code.leading_zeros()).max(1).div_ceil(4);
        // Integer coordinates use the same clipped cell dimensions as the combined outline.
        let height = u32::from(self.height);
        std::iter::once(0).chain((0..digits).filter_map(move |position| {
            // The original mini alphabet omits rows that would touch the bottom border.
            if self.width < 9 || 2 + (position / 2) * 6 + 5 >= height {
                return None;
            }
            let nibble = (code >> ((digits - position - 1) * 4)) & 15;
            // Reserve zero for the border; the remaining bits encode position and value.
            Some(1 + position * 16 + nibble)
        }))
    }

    /// Constructs one reusable component in the same cell-height em space as `tofu`.
    pub fn tofu_component(self, id: u32) -> Result<BuiltinInk, String> {
        // IDs encode eight possible nibble positions; rejecting larger values bounds shifts.
        if id > 128 {
            return Err("invalid tofu component identity".into());
        }
        // A cold component creates its path once; atlas hits bypass this constructor.
        let mut pen = PixelPen {
            path: BezPath::new(),
            height: f64::from(self.height),
        };
        if id == 0 {
            symbols::tofu_border(&mut pen, self);
        } else {
            // Subtract the border reservation before separating position from nibble.
            let position = (id - 1) / 16;
            let digit = char::from_digit((id - 1) & 15, 16).ok_or("invalid tofu digit")?;
            // Positions remain relative to the complete cell, never fitted individually.
            symbols::draw_digit(
                &mut pen,
                self,
                digit,
                1 + (position % 2) as i32 * 4,
                2 + (position / 2) as i32 * 6,
            );
        }
        Ok(BuiltinInk {
            path: pen.path,
            alpha: 255,
        })
    }
}
