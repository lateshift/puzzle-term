//! Typed settings applied at the Session seam (§14.3).
//!
//! Only the settings implemented by the current terminal composition
//! enter SessionConfig. Font size remains with the host-owned TextContext;
//! color themes acquire their own resolver in M6.6.
use super::{BlinkMode, Settings};
use crate::session::{
    OnExit, OptionAsAlt, SessionConfig,
    cursor_settings::{CursorBlink, CursorColor, CursorSettings, CursorStyle},
};
use std::time::Duration;

impl Settings {
    /// Builds the session's initial parser, history and input policies.
    ///
    /// Grid dimensions and TERM come from the platform at spawn time; the
    /// schema must not replace those physical facts with its defaults.
    #[must_use]
    pub fn session_config(&self) -> SessionConfig {
        // Values entered through config.json, CLI or a future web adapter
        // have already passed the same typed validation and range clamp.
        SessionConfig {
            // VT owns history and evicts only rows behind the visible grid;
            // these limits are independent of the GPU's glyph memory cap.
            scrollback_lines: self.scrollback_lines as usize,
            scrollback_bytes: self.scrollback_max_bytes as usize,
            // Parsing remains bounded on the caller's clock, preserving a
            // responsive host even when a program writes indefinitely.
            parse_budget: Duration::from_millis(u64::from(self.performance_parse_budget_ms)),
            // Mode 2027 starts from this preference. Applications may still
            // change its live value until RIS restores the configured one.
            grapheme_width_unicode: self.unicode_grapheme_width == "unicode",
            // The adapter receives the same Option policy before the first
            // key; printable German-layout chords then keep their text.
            option_as_alt: self.option_as_alt(),
            // The host's display pacer also receives this duration; Session
            // keeps it in the shared configuration for browser parity.
            render_settle: Duration::from_millis(u64::from(self.render_settle_ms)),
            // A normal child exit obeys this preference. The short-failure
            // threshold below may still hold an abnormal diagnostic.
            on_exit: if self.session_on_exit == "hold" {
                OnExit::Hold
            } else {
                OnExit::Close
            },
            abnormal_exit_ms: u64::from(self.session_abnormal_exit_ms),
            // Cursor defaults are separate from VT's stored DECSCUSR code,
            // allowing DECRQSS to answer the application's last request.
            cursor: self.cursor_settings(),
            ..SessionConfig::default()
        }
    }

    /// Converts the four validated cursor fields without exposing JSON data.
    ///
    /// Rendering reads this concrete copy on a frame turn, never a Value
    /// map or a string comparison for each terminal cell (P1).
    #[must_use]
    pub fn cursor_settings(&self) -> CursorSettings {
        // The schema accepts these four stable names. The default branch
        // still handles a Settings struct built directly in Rust by a test.
        let style = match self.cursor_style.as_str() {
            "bar" => CursorStyle::Bar,
            "underline" => CursorStyle::Underline,
            "block_hollow" => CursorStyle::BlockHollow,
            _ => CursorStyle::Block,
        };
        // Auto preserves DECSCUSR and mode ?12; forced booleans override
        // both without changing the application's terminal mode reports.
        let blink = match self.cursor_blink {
            BlinkMode::Auto => CursorBlink::Auto,
            BlinkMode::On => CursorBlink::On,
            BlinkMode::Off => CursorBlink::Off,
        };
        // The schema checked the hexadecimal spelling. A fallback still
        // guards callers who construct a Settings struct directly in Rust.
        let color = CursorColor::parse(&self.cursor_color).unwrap_or(CursorColor::Theme);
        CursorSettings {
            style,
            blink,
            // A disallowed application shape is retained in VT for query
            // replies but cannot affect the shape or blink drawn on screen.
            allow_app_style: self.cursor_allow_app_style,
            color,
        }
    }

    /// Converts the validated per-side Option policy for the host adapter.
    ///
    /// A right-only or left-only choice must not be collapsed into one Alt
    /// bit before winit's side metadata is considered (§6.8, D-18).
    #[must_use]
    pub fn option_as_alt(&self) -> OptionAsAlt {
        match self.keyboard_option_as_alt.as_str() {
            "left" => OptionAsAlt::Left,
            "right" => OptionAsAlt::Right,
            "both" => OptionAsAlt::Both,
            _ => OptionAsAlt::None,
        }
    }
}
