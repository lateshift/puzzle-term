//! Change-only host chrome colours from the same defaults/overrides as rendering.
//!
//! The Session owner samples its table; hosts receive copied bytes through tick.
//! Theme animation uses its sampled defaults, while OSC overrides retain precedence.
//! Luminance work runs only on an actual colour change, never on steady output.
use super::{Session, host_events::HostEvent};
use crate::vt::color_table::{ColorSlot, DynamicColor};

impl Session {
    /// Publish once on observation, then only when effective default RGB changes.
    pub(super) fn collect_host_colors(&mut self) {
        let theme = self.theme_state.rendered().or_else(|| self.active_theme());
        // Unthemed VT fallback matches the renderer's original black/gray defaults.
        let fallback = theme.map_or(([0; 3], [229; 3]), |theme| {
            (theme.background, theme.foreground)
        });
        // During a transition, the renderer samples theme defaults but preserves
        // application overrides; raw VT query defaults already describe its target.
        let resolve = |role, fallback| {
            let slot = ColorSlot::Dynamic(role);
            if self.theme_state.transitioning()
                && self.terminal.colors.override_value(slot).is_none()
            {
                fallback
            } else {
                // Explicitly dynamic kitty roles use the same host fallback as cells.
                self.terminal.colors.get(slot).rgb().unwrap_or(fallback)
            }
        };
        let background = resolve(DynamicColor::Background, fallback.0);
        let foreground = resolve(DynamicColor::Foreground, fallback.1);
        // Ordinary output and cell SGR cannot change these six default-colour bytes.
        if self.host_colors == Some((background, foreground)) {
            return;
        }
        // Remember before queuing: repeated outbox reads must not duplicate this fact.
        self.host_colors = Some((background, foreground));
        // Compare black/white contrast in linear light. This derives the dark/light
        // boundary from relative luminance rather than the theme's metadata label.
        let dark = crate::theme::color::contrast(background, [255; 3])
            >= crate::theme::color::contrast(background, [0; 3]);
        self.host_events.push_back(HostEvent::Colors {
            background,
            foreground,
            dark,
        });
    }
}
