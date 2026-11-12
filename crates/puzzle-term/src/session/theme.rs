//! Applies host appearance to a neutral session's retained terminal colors.
//!
//! The registry resolves user themes before any VT override is cleared. A
//! broken custom theme therefore leaves the old screen and cursor intact.
//! Once a change is valid, OSC 4/10/11/12 overrides expire together so an
//! application's later OSC query sees the newly selected theme (§14.7).
//!
//! A named theme is resolved before it enters the frame builder. VT stores
//! only tagged cell colors and the three current OSC query defaults; it
//! never parses user JSON or consults the host's dark-mode API. That keeps
//! output parsing deterministic under a recording replay.
//!
//! There are two separate notions of appearance here. `system` is the last
//! fact reported by the platform, while `mode` may force either member of
//! the user's configured pair. A forced mode must not erase the system fact:
//! switching back to `auto` should immediately choose the current host
//! appearance, even if the OS changed while the forced mode was active.
//!
//! The registry is replaced as a unit on a valid settings transaction.
//! Registration checks syntax, and resolution checks the selected name,
//! its ancestors and cycles. A malformed config may still be retained in
//! the settings store for editing, but it cannot half-recolor the screen.
//! The inactive pair member is resolved when selected, because it might
//! name a custom theme registered by a later config edit.
//!
//! Cells keep palette indices rather than theme RGB. When a new theme is
//! installed, the frame builder increments its style epoch: cached row
//! geometry is recolored on the next build without changing text advances
//! or rewriting the primary and alternate VT screens. The surface clear
//! and padding use the same background as default-background cells.
//!
//! OSC colors are application overrides. They are deliberately discarded
//! whenever the resolved theme changes, so the application can reissue
//! them after the optional mode-2031 report. An unrelated settings reload
//! that leaves the resolved theme equal keeps those overrides intact.
use super::Session;
use crate::{
    frame_builder::policy::FramePolicy,
    platform::Services,
    settings::{Settings, ThemeMode},
    theme::{Appearance, Theme, ThemeRegistry},
    vt::Terminal,
};
use puzzle_bus::Value;
use std::time::Duration;
use web_time::Instant;

/// One finite appearance fade sampled only at host-provided frame times.
struct Transition {
    /// Last visible colors when this switch started.
    from: Theme,
    /// Fully resolved destination, already installed for OSC queries.
    to: Theme,
    /// Injected clock value of the appearance change.
    started: Instant,
    /// Validated user duration in wall-clock time.
    duration: Duration,
}

/// Per-session theme pair and the last appearance reported by the host.
pub(super) struct ThemeState {
    /// Custom definitions validated before each settings transaction.
    registry: ThemeRegistry,
    /// Explicit mode or host-following selection.
    mode: ThemeMode,
    /// Light-side selector, which may name a custom theme.
    light: String,
    /// Dark-side selector, which may name a custom theme.
    dark: String,
    /// Last host appearance, dark if the platform cannot report one.
    system: Appearance,
    /// Active resolved tokens for introspection and change detection.
    active: Option<Theme>,
    /// Last colors sent to the frame builder, even during a fade.
    rendered: Option<Theme>,
    /// A nonzero setting chooses a finite OKLab fade on future switches.
    transition_ms: u32,
    /// In-progress render colors, independent of immediate OSC defaults.
    transition: Option<Transition>,
}

impl Default for ThemeState {
    /// Starts with the same dark pair as the settings schema.
    fn default() -> Self {
        // The neutral core has no access to a host preference at creation.
        // Dark is the §14.7 fallback until the platform submits its fact.
        Self {
            registry: ThemeRegistry::default(),
            mode: ThemeMode::Auto,
            light: "puzzle-light".into(),
            dark: "puzzle-dark".into(),
            system: Appearance::Dark,
            active: None,
            rendered: None,
            transition_ms: 0,
            transition: None,
        }
    }
}

impl ThemeState {
    /// Resolves every custom definition before adopting a settings snapshot.
    pub(super) fn configure(&mut self, settings: &Settings) -> Result<Option<Theme>, String> {
        // Build a replacement map first; modifying the active registry as
        // definitions arrive could leave a half-valid inheritance graph.
        let mut registry = ThemeRegistry::default();
        // The settings schema carries arbitrary custom maps as Value, so
        // this owning subsystem still has to check their root shape.
        let Value::Map(definitions) = &settings.themes else {
            return Err("themes must be an object".into());
        };
        // Registration is independent of parent order; resolution below
        // detects missing parents and cycles before live colors change.
        for (name, value) in definitions {
            // A child may precede its parent in the lexical map order.
            // Registering data alone does not require a resolved parent.
            registry.register(name, value)?;
        }
        // Resolve the side selected by `auto` or an explicit mode only
        // after all custom names are present in the candidate registry.
        let mode = settings.theme_mode;
        let name = selected_name(
            mode,
            self.system,
            &settings.theme_light,
            &settings.theme_dark,
        );
        let theme = registry.resolve(name)?;
        // All fallible work is complete. These assignments make the new
        // pair and registry visible to the next appearance notification.
        self.registry = registry;
        self.mode = mode;
        self.light = settings.theme_light.clone();
        self.dark = settings.theme_dark.clone();
        self.transition_ms = settings.theme_transition_ms;
        let changed = self.active.as_ref() != Some(&theme);
        // Equality includes colors as well as name: editing one custom
        // theme in place must rebuild its retained colored row instances.
        self.active = Some(theme.clone());
        Ok(changed.then_some(theme))
    }

    /// Returns a new effective theme only when host appearance changed it.
    pub(super) fn system_changed(
        &mut self,
        appearance: Appearance,
    ) -> Result<Option<Theme>, String> {
        // Duplicate native theme callbacks must not trigger a full-row
        // rebuild, clear OSC colors or send redundant PTY notifications.
        if self.system == appearance {
            return Ok(None);
        }
        // The candidate is resolved before updating `system`; a broken
        // inactive theme can then be repaired and retried on the next fact.
        let name = selected_name(self.mode, appearance, &self.light, &self.dark);
        let theme = self.registry.resolve(name)?;
        self.system = appearance;
        // A forced theme may stay identical while the OS mode changes.
        // Remember the system fact but leave the visible frame untouched.
        if self.active.as_ref() == Some(&theme) {
            return Ok(None);
        }
        self.active = Some(theme.clone());
        Ok(Some(theme))
    }

    /// Names the active pair member for diagnostics and gallery snapshots.
    pub(super) fn active(&self) -> Option<&Theme> {
        self.active.as_ref()
    }

    /// Preserves the last visible fade sample when asynchronous fonts arrive.
    pub(super) fn rendered(&self) -> Option<&Theme> {
        // The destination used by OSC queries can differ during a transition.
        self.rendered.as_ref()
    }

    /// Starts from the last displayed colors, or switches instantly at boot.
    pub(super) fn begin(&mut self, theme: &Theme, now: Instant) {
        // A new OS mode during an existing fade begins at its last sampled
        // color, so repeated switches do not jump back to an old endpoint.
        if self.transition_ms > 0
            && let Some(from) = self.rendered.as_ref()
            && from != theme
        {
            self.transition = Some(Transition {
                from: from.clone(),
                to: theme.clone(),
                started: now,
                duration: Duration::from_millis(u64::from(self.transition_ms)),
            });
        } else {
            // There is no previous frame to fade from on startup. A zero
            // duration is also the default, exact palette switch (§14.7).
            self.transition = None;
            self.rendered = Some(theme.clone());
        }
    }

    /// Samples the fade at a frame deadline supplied by the host clock.
    pub(super) fn sample(&mut self, now: Instant) -> Option<Theme> {
        let transition = self.transition.as_ref()?;
        let elapsed = now.saturating_duration_since(transition.started);
        let amount = elapsed.as_secs_f64() / transition.duration.as_secs_f64();
        let theme = transition.from.mix_to(&transition.to, amount);
        self.rendered = Some(theme.clone());
        if amount >= 1.0 {
            // The last frame uses the exact source-backed target bytes.
            self.transition = None;
        }
        Some(theme)
    }

    /// Tells the scheduler whether another color frame is due.
    pub(super) fn transitioning(&self) -> bool {
        self.transition.is_some()
    }
}

/// Selects the pair member after an explicit mode overrides the host.
fn selected_name<'a>(
    mode: ThemeMode,
    system: Appearance,
    light: &'a str,
    dark: &'a str,
) -> &'a str {
    // This decision uses the host only in Auto. The named theme's own
    // metadata is used later for OSC reports and CSS color-scheme.
    match mode {
        ThemeMode::Light => light,
        ThemeMode::Dark => dark,
        ThemeMode::Auto if system == Appearance::Light => light,
        ThemeMode::Auto => dark,
    }
}

/// Commits a valid theme as one render and VT override transaction.
pub(super) fn install(
    theme: &Theme,
    terminal: &mut Terminal,
    builder: Option<&mut FramePolicy>,
    services: &mut Services,
) {
    // One transaction clears every override and installs query/drawing defaults.
    theme.install_terminal_colors(&mut terminal.colors);
    terminal.appearance_dark = theme.appearance == Appearance::Dark;
    if let Some(builder) = builder {
        // Bumping the style epoch recolors default and indexed instances;
        // Slug glyph preparation and cell placement remain reusable.
        builder.set_theme(theme);
    }
    // Both callers reach this transaction only after the resolved theme
    // changes. Mode 2031 reports that fact regardless of whether it came
    // from OS appearance, a profile or a config/CLI settings update.
    if terminal.modes.get(true, 2031) == Some(true) {
        // Unsolicited reports spend the same program-reply credit as explicit queries.
        terminal
            .replies
            .extend_from_slice(theme.appearance.report());
    }
    // A quiet application still needs one frame after an OS theme change.
    services.waker.request_frame();
}

impl Session {
    /// Resolves a validated theme pair after a live config reload.
    pub(super) fn apply_theme_settings(&mut self, settings: &Settings, now: Instant) {
        match self.theme_state.configure(settings) {
            Ok(Some(theme)) => {
                self.theme_state.begin(&theme, now);
                // The selected palette is now valid. Commit the VT query
                // defaults and row-cache epoch together before observers run.
                install(
                    &theme,
                    &mut self.terminal,
                    self.frame_policy.as_mut(),
                    &mut self.services,
                );
                self.needs_frame = true;
                // Subscribers receive a fact after the screen state changes,
                // so a callback can inspect the new resolved token set.
                self.bus.publish(crate::messages::Event::ThemeChanged {
                    name: theme.name,
                    appearance: theme.appearance,
                });
            }
            Ok(None) => {}
            // Settings reports already preserve the invalid user value for
            // repair; rendering continues with the last valid theme.
            Err(error) => tracing::warn!(event="settings.theme_rejected", %error),
        }
        self.flush_parser_replies();
    }

    /// Returns resolved tokens for a host-owned gallery or CSS adapter.
    #[must_use]
    pub fn active_theme(&self) -> Option<&Theme> {
        self.theme_state.active()
    }

    /// Applies one transient color step before a snapshot is captured.
    pub(super) fn advance_theme(&mut self, now: Instant) {
        let Some(theme) = self.theme_state.sample(now) else {
            return;
        };
        if let Some(builder) = self.frame_policy.as_mut() {
            // Tagged cells are recolored; the VT's OSC query defaults
            // remain on the destination theme throughout the animation.
            builder.set_theme(&theme);
        }
        self.needs_frame = true;
        if self.theme_state.transitioning() {
            // Eight-millisecond turns cover 120-Hz displays; each platform
            // still coalesces these requests to its actual refresh source.
            self.services.waker.wake_at(now + Duration::from_millis(8));
        }
    }
}
