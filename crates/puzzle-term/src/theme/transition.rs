//! OKLab interpolation of resolved theme tokens (§14.7).
//!
//! A transition changes rendered colors while the named target theme and
//! its OSC query defaults switch at once. Mixing after source resolution
//! preserves exact official palette endpoints: generated JSON bytes are
//! never rewritten to make an animation look smoother.
//!
//! The amount is a fraction supplied by Session's injected frame clock.
//! Intermediate colors are transient render data, never settings or VT
//! state. Alpha is linear because it represents coverage, whereas RGB uses
//! the perceptual OKLab path defined for theme derivation in §14.6.

use super::{Theme, UiTokens, color};

impl Theme {
    /// Blends terminal and UI colors, retaining the target's identity.
    pub(crate) fn mix_to(&self, target: &Self, amount: f64) -> Self {
        // Exact endpoints avoid a round-trip through OKLab at the first
        // and last frame, so an official source color stays byte exact.
        if amount <= 0.0 {
            return self.clone();
        }
        if amount >= 1.0 {
            return target.clone();
        }
        let mix = |a, b| color::mix(a, b, amount);
        // Indexed cells retain their slot number through a theme switch.
        // Mixing corresponding slots lets them fade without VT rewrites.
        let mut palette = [[0; 3]; 16];
        for (index, slot) in palette.iter_mut().enumerate() {
            *slot = mix(self.palette[index], target.palette[index]);
        }
        // The named target and its light/dark identity change immediately;
        // only the draw colors take the configured time to catch up.
        Self {
            name: target.name.clone(),
            display_name: target.display_name.clone(),
            appearance: target.appearance,
            credit: target.credit.clone(),
            // Default-tagged cells and the surface clear share this pair.
            // Mixing both avoids a flash at the grid's padded edge.
            foreground: mix(self.foreground, target.foreground),
            background: mix(self.background, target.background),
            cursor: mix(self.cursor, target.cursor),
            cursor_text: mix(self.cursor_text, target.cursor_text),
            // Selection ink and fill share a progress fraction so one does
            // not reach the new palette while the other remains old.
            selection_background: mix(self.selection_background, target.selection_background),
            selection_foreground: target
                .selection_foreground
                .map(|ink| mix(self.selection_foreground.unwrap_or(self.foreground), ink)),
            palette,
            // Overlay chrome receives the same time fraction as cell ink.
            ui: self.ui.mix_to(&target.ui, amount),
        }
    }
}

impl UiTokens {
    /// Blends every overlay color so chrome and terminal cells stay paired.
    fn mix_to(&self, target: &Self, amount: f64) -> Self {
        let mix = |a, b| color::mix(a, b, amount);
        // The thumb stores opacity beside its RGB color; interpreting its
        // alpha as a fourth OKLab axis would change physical coverage.
        let thumb = mix(
            [
                self.scrollbar_thumb[0],
                self.scrollbar_thumb[1],
                self.scrollbar_thumb[2],
            ],
            [
                target.scrollbar_thumb[0],
                target.scrollbar_thumb[1],
                target.scrollbar_thumb[2],
            ],
        );
        // Alpha is a linear opacity. Perceptual color math applies only
        // to the three RGB components under it.
        let thumb_alpha = (f64::from(self.scrollbar_thumb[3]) * (1.0 - amount)
            + f64::from(target.scrollbar_thumb[3]) * amount)
            .round() as u8;
        Self {
            // Interactive accents, scrollbar and search highlights fade
            // from one coherent appearance to the other at one clock tick.
            accent: mix(self.accent, target.accent),
            scrollbar_thumb: [thumb[0], thumb[1], thumb[2], thumb_alpha],
            scrollbar_track: mix(self.scrollbar_track, target.scrollbar_track),
            search_match: mix(self.search_match, target.search_match),
            search_current: mix(self.search_current, target.search_current),
            link: mix(self.link, target.link),
            // Status tokens retain their semantic roles throughout the
            // switch; neither success nor error borrows ANSI slot meaning.
            zone_ok: mix(self.zone_ok, target.zone_ok),
            zone_error: mix(self.zone_error, target.zone_error),
            badge_background: mix(self.badge_background, target.badge_background),
            badge_foreground: mix(self.badge_foreground, target.badge_foreground),
            tooltip_background: mix(self.tooltip_background, target.tooltip_background),
            tooltip_foreground: mix(self.tooltip_foreground, target.tooltip_foreground),
            // Badges, pills and tooltips blend their text and backing
            // independently so authored endpoints remain exact.
            pill_background: mix(self.pill_background, target.pill_background),
            pill_foreground: mix(self.pill_foreground, target.pill_foreground),
            bell_flash: mix(self.bell_flash, target.bell_flash),
            ghost_text: mix(self.ghost_text, target.ghost_text),
            preedit_underline: mix(self.preedit_underline, target.preedit_underline),
            gutter: mix(self.gutter, target.gutter),
        }
    }
}
