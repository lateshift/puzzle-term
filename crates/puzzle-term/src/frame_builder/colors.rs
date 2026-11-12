//! Theme-time resolution of VT tagged colors into frame colors (§10.5).
//!
//! Cells retain palette indices so OSC 4 can recolor old text. A style
//! change invalidates cached instances; the row builder then resolves each
//! color against the current palette before it writes the GPU instance.
//! The policy starts with Puzzle Dark and accepts resolved theme tokens
//! without rewriting cells. The clear and default cell tags resolve alike.
//!
//! The foreground/background order matters. A palette tag is looked up
//! first, because a later OSC 4 can replace a palette slot without changing
//! stored cells. Bold-bright promotion applies only to the first eight
//! indexed foreground slots. Truecolor and default ink keep their values.
//! SGR inverse then swaps the resolved pair, and DECSCNM swaps the whole
//! screen's pair. Their XOR means enabling both restores normal ordering.
//! Dim is a foreground effect after swaps: its result moves toward the
//! actual background the user sees, even inside reverse-video cells.
//!
//! Minimum contrast is a rendering policy rather than a grid mutation.
//! WCAG measures linear-light relative luminance, though the default D-27
//! compositor mixes encoded sRGB values. This module uses luminance only
//! to find a sufficient ink color; the shader's blending remains native.
//! With ratio 1 the contrast path returns immediately, so default frames
//! do not run a binary search for every terminal cell.
use crate::frame_snapshot::PaintState;
use crate::session::cursor_settings::CursorColor;
use crate::theme::Theme;
use puzzle_vt::cell::{Attrs, Cell, PackedColor};
use puzzle_vt::color_table::{ColorSlot, ColorValue, DynamicColor};

/// Render-time palette and contrast options shared by every visible cell.
#[derive(Clone, PartialEq)]
pub(crate) struct ColorPolicy {
    /// Theme foreground before terminal OSC 10 overrides.
    foreground: [u8; 3],
    /// Theme background before terminal OSC 11 overrides.
    background: [u8; 3],
    /// Ordinary and current query paper follows semantic UI theme tokens.
    search: [[u8; 3]; 2],
    /// ANSI slots 0–15 from the selected theme's official palette.
    palette: [[u8; 3]; 16],
    /// Whether SGR bold turns palette slots 0–7 into 8–15.
    bold_is_bright: bool,
    /// Minimum WCAG luminance ratio; one disables adjustment.
    minimum_contrast: f32,
}

impl Default for ColorPolicy {
    /// Preserves the M6.3 baseline until the host applies typed settings.
    fn default() -> Self {
        // Historical frame goldens instantiate a Session with no settings.
        // Live windows immediately select generated Puzzle Dark before draw.
        Self {
            foreground: [229; 3],
            background: [14, 18, 26],
            search: [
                crate::theme::Theme::builtin("puzzle-dark")
                    .map_or([51, 68, 85], |t| t.ui.search_match),
                crate::theme::Theme::builtin("puzzle-dark")
                    .map_or([119, 85, 51], |t| t.ui.search_current),
            ],
            palette: std::array::from_fn(|index| palette(index as u8)),
            bold_is_bright: false,
            minimum_contrast: 1.0,
        }
    }
}

impl ColorPolicy {
    /// Background assembly applies this token without changing row caches.
    pub(super) fn selection_background(&self, paint: &PaintState) -> u32 {
        // Selection colour travels with the captured rows: consulting the live VT
        // here could mix a new OSC value with cells from an older publication.
        rgba(
            paint
                .colors
                .get(ColorSlot::Dynamic(DynamicColor::SelectionBackground))
                .rgb()
                .unwrap_or_else(|| self.foreground(paint)),
        )
    }
    /// Chooses the strong current match or the subtle ordinary match token.
    pub(super) fn search_background(&self, current: bool) -> u32 {
        rgba(self.search[usize::from(current)])
    }
    /// Adjusts selected ink against its actual visible selection surface.
    pub(super) fn selection_ink(&self, paint: &PaintState, original: u32) -> u32 {
        let slot = ColorSlot::Dynamic(DynamicColor::SelectionForeground);
        // An explicit kitty dynamic override requests paper-coloured ink, whereas
        // an absent theme token preserves each cell's resolved SGR foreground.
        let fallback = if paint.colors.override_value(slot) == Some(ColorValue::Dynamic) {
            self.background(paint)
        } else {
            [
                original as u8,
                (original >> 8) as u8,
                (original >> 16) as u8,
            ]
        };
        let rgb = paint.colors.get(slot).rgb().unwrap_or(fallback);
        rgba(enforce_contrast(
            rgb,
            rgb_bytes(self.selection_background(paint)),
            self.minimum_contrast,
        ))
    }
    /// Avoids partitioning text batches when selection cannot change its ink.
    pub(super) fn selection_recolors(&self, paint: &PaintState) -> bool {
        // Only a fixed colour, explicit dynamic override or contrast correction
        // requires splitting glyph instances at the selection boundary.
        let slot = ColorSlot::Dynamic(DynamicColor::SelectionForeground);
        paint.colors.get(slot).rgb().is_some()
            || paint.colors.override_value(slot).is_some()
            || self.minimum_contrast > 1.0
    }

    /// Resolves the default ink from the sealed table, including OSC 10/21.
    fn foreground(&self, paint: &PaintState) -> [u8; 3] {
        // Dynamic OSC 21 ink has no fixed RGB; retain the host's ordinary ink.
        paint
            .colors
            .get(ColorSlot::Dynamic(DynamicColor::Foreground))
            .rgb()
            .unwrap_or(self.foreground)
    }
    /// Resolves default paper for both cells and swapchain clear.
    fn background(&self, paint: &PaintState) -> [u8; 3] {
        // The fallback also covers explicitly undefined kitty background values.
        paint
            .colors
            .get(ColorSlot::Dynamic(DynamicColor::Background))
            .rgb()
            .unwrap_or(self.background)
    }

    /// Returns the selected theme's redrawn cursor glyph color.
    pub(super) fn cursor_text(&self, paint: &PaintState) -> [u8; 3] {
        // A dynamic cursor-text role uses the current paper for readable inversion.
        paint
            .colors
            .get(ColorSlot::Dynamic(DynamicColor::CursorText))
            .rgb()
            .unwrap_or_else(|| self.background(paint))
    }

    /// Replaces semantic colors while leaving render-only options intact.
    pub(crate) fn set_theme(&mut self, theme: &Theme) {
        self.foreground = theme.foreground;
        self.background = theme.background;
        self.palette = theme.palette;
        self.search = [theme.ui.search_match, theme.ui.search_current];
    }

    /// Chooses the rectangle and glyph colors after app and cell overrides.
    pub(super) fn cursor_pair(
        &self,
        paint: &PaintState,
        setting: CursorColor,
        cell: Cell,
    ) -> ([u8; 3], [u8; 3]) {
        // OSC 12 is an application override until OSC 112 or RIS removes
        // it; only then may a user's inverse or fixed choice take effect.
        let pair = if let Some(value) = paint
            .colors
            .override_value(ColorSlot::Dynamic(DynamicColor::Cursor))
        {
            // Undefined cursor colour follows cell ink/paper rather than guessing
            // one static RGB; a concrete OSC override still outranks user settings.
            if let Some(osc) = value.rgb() {
                (osc, self.cursor_text(paint))
            } else {
                self.resolve(paint, cell.fg, cell.bg, cell.attrs)
            }
        } else {
            match setting {
                CursorColor::Theme => (
                    paint
                        .colors
                        .get(ColorSlot::Dynamic(DynamicColor::Cursor))
                        .rgb()
                        .unwrap_or_else(|| self.foreground(paint)),
                    self.cursor_text(paint),
                ),
                CursorColor::Fixed(rgb) => (rgb, self.cursor_text(paint)),
                CursorColor::Inverse => {
                    let (foreground, background) =
                        self.resolve(paint, cell.fg, cell.bg, cell.attrs);
                    (foreground, background)
                }
            }
        };
        // The ordinary contrast rule adjusts text against its visible
        // surface; here the surface is the cursor rectangle itself.
        (
            pair.0,
            enforce_contrast(pair.1, pair.0, self.minimum_contrast),
        )
    }

    /// Returns the WebGPU clear components for default-background cells.
    pub(super) fn clear(&self, paint: &PaintState) -> [f32; 4] {
        // The swapchain wants 0..1 components, whereas terminal palette
        // entries and GPU instance colors are byte-valued sRGB.
        // OSC 11 changes the active default background without editing
        // stored grid cells; it must also recolor grid padding and clear.
        let background = self.background(paint);
        [
            f32::from(background[0]) / 255.0,
            f32::from(background[1]) / 255.0,
            f32::from(background[2]) / 255.0,
            1.0,
        ]
    }

    /// Enables two render-only theme rules for later settings application.
    pub(crate) fn set_options(&mut self, bold_is_bright: bool, minimum_contrast: f32) {
        self.bold_is_bright = bold_is_bright;
        // A hostile nonfinite value cannot reach the luminance search;
        // clamping leaves the default ratio unchanged for invalid input.
        self.minimum_contrast = if minimum_contrast.is_finite() {
            minimum_contrast.clamp(1.0, 21.0)
        } else {
            1.0
        };
    }

    /// Applies palette, inverse, dim and contrast rules in §10.5 order.
    pub(super) fn resolve(
        &self,
        paint: &PaintState,
        fg: PackedColor,
        bg: PackedColor,
        attrs: Attrs,
    ) -> ([u8; 3], [u8; 3]) {
        let (fg, bg) = self.resolve_uncontrasted(paint, fg, bg, attrs);
        (enforce_contrast(fg, bg, self.minimum_contrast), bg)
    }
    /// Preserves pre-contrast ink so selection is applied before rule 7.
    pub(super) fn resolve_uncontrasted(
        &self,
        paint: &PaintState,
        fg: PackedColor,
        bg: PackedColor,
        attrs: Attrs,
    ) -> ([u8; 3], [u8; 3]) {
        // OSC 10 and 11 override only the default tags. Indexed and RGB
        // cells remain independent of these terminal-wide defaults.
        let fg_default = self.foreground(paint);
        let bg_default = self.background(paint);
        // xterm's bold-is-bright convention changes the palette index,
        // not the alpha or RGB channels of a literal truecolor cell.
        let fg = if self.bold_is_bright
            && attrs.contains(Attrs::BOLD)
            && fg.0 >> 24 == 1
            && (fg.0 as u8) < 8
        {
            PackedColor::palette(fg.0 as u8 + 8)
        } else {
            fg
        };
        let mut foreground = self.resolve_color(paint, fg, fg_default);
        let mut background = self.resolve_color(paint, bg, bg_default);
        // Reverse video is a screen mode; inverse is a cell attribute.
        // Both toggles are involutions, so two active toggles cancel.
        if attrs.contains(Attrs::INVERSE) ^ paint.reverse_video {
            std::mem::swap(&mut foreground, &mut background);
        }
        if attrs.contains(Attrs::DIM) {
            // SGR 2 leaves the background untouched and reduces only ink
            // contrast. This runs after inversion because ink can be an
            // old background color in an inverse cell.
            for channel in 0..3 {
                foreground[channel] = mix(background[channel], foreground[channel], 0.5);
            }
        }
        // A minimum-contrast theme rule also belongs after inverse and
        // dim: earlier adjustment could be undone by either operation.
        (foreground, background)
    }
    /// Resolves one indexed or literal color after OSC overrides.
    pub(super) fn resolve_color(
        &self,
        paint: &PaintState,
        color: PackedColor,
        default: [u8; 3],
    ) -> [u8; 3] {
        // The high byte is a tag, so the low byte of truecolor is never
        // interpreted as an accidental 256-color palette index (§6.2).
        match color.0 >> 24 {
            1 => {
                let index = color.0 as u8;
                // OSC 4 overrides win over the theme's xterm palette, including
                // an override installed after this cell was originally printed.
                paint
                    .colors
                    .get(ColorSlot::Palette(index))
                    .rgb()
                    .unwrap_or_else(|| self.palette(index))
            }
            2 => [(color.0 >> 16) as u8, (color.0 >> 8) as u8, color.0 as u8],
            _ => default,
        }
    }

    /// Keeps the authored sixteen ANSI slots ahead of the xterm color cube.
    fn palette(&self, index: u8) -> [u8; 3] {
        if index < 16 {
            self.palette[index as usize]
        } else {
            palette(index)
        }
    }
}

/// Uses the VT table's unconfigured defaults when no host theme was installed.
fn palette(index: u8) -> [u8; 3] {
    // One xterm definition serves queries and rendering, including grey/cube slots.
    puzzle_vt::color_table::palette(index)
}

/// Blends an encoded sRGB channel as the D-27 native policy specifies.
fn mix(bg: u8, fg: u8, share: f32) -> u8 {
    // sRGB bytes stay encoded for the native D-27 mode; an opt-in linear
    // compositor is a separate settings decision, not a palette change.
    (f32::from(bg) * (1.0 - share) + f32::from(fg) * share).round() as u8
}

/// Packs straight sRGB bytes for the rect and Slug shaders.
pub(super) fn rgba(color: [u8; 3]) -> u32 {
    // Little-endian GPU vertex fetch sees the least significant byte as
    // red, matching the shader's packed `unpack4x8unorm` channel order.
    0xff00_0000 | (u32::from(color[2]) << 16) | (u32::from(color[1]) << 8) | u32::from(color[0])
}

/// Adjusts ink brightness only if the requested contrast exceeds WCAG's ratio.
fn enforce_contrast(fg: [u8; 3], bg: [u8; 3], minimum: f32) -> [u8; 3] {
    // A ratio of one is the §14.5 default, and every color pair already
    // meets it. This also avoids powf calls on the normal frame path.
    if minimum <= 1.0 {
        return fg;
    }
    let foreground = luminance(fg);
    let background = luminance(bg);
    // WCAG adds 0.05 to both luminances so the ratio is defined even
    // against absolute black, then divides the lighter by the darker.
    let ratio = (foreground.max(background) + 0.05) / (foreground.min(background) + 0.05);
    if ratio >= minimum {
        return fg;
    }
    // Brighter ink on a dark surface and darker ink on a light one preserve
    // the original color as long as possible before reaching the ratio.
    // Binary search chooses the weakest such adjustment: a full white or
    // black replacement would unnecessarily erase syntax highlighting.
    let target = if background < 0.5 { [255; 3] } else { [0; 3] };
    let mut low = 0.0;
    let mut high = 1.0;
    // Twelve bisections resolve the byte output more finely than one
    // channel level; further iterations would not change the GPU color.
    for _ in 0..12 {
        let middle = (low + high) * 0.5;
        let candidate = [
            mix(fg[0], target[0], middle),
            mix(fg[1], target[1], middle),
            mix(fg[2], target[2], middle),
        ];
        let value = luminance(candidate);
        // Search only the segment from the source to the contrast target.
        // Luminance is monotone along this same-direction channel blend.
        let achieved = (value.max(background) + 0.05) / (value.min(background) + 0.05);
        if achieved >= minimum {
            high = middle;
        } else {
            low = middle;
        }
    }
    [
        mix(fg[0], target[0], high),
        mix(fg[1], target[1], high),
        mix(fg[2], target[2], high),
    ]
}

/// WCAG relative luminance uses linearized sRGB channels (§14.6).
fn luminance(rgb: [u8; 3]) -> f32 {
    // WCAG's transfer function has a linear toe for dark components and
    // a 2.4-power segment elsewhere; applying weights to encoded bytes
    // would overstate the brightness of low-saturation dark text.
    let channel = |value: u8| {
        let c = f32::from(value) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2])
}

#[cfg(test)]
/// Checks the render-time policy without changing VT color storage.
mod tests;

/// Unpacks resolved straight sRGB bytes for selection contrast.
fn rgb_bytes(color: u32) -> [u8; 3] {
    [color as u8, (color >> 8) as u8, (color >> 16) as u8]
}
