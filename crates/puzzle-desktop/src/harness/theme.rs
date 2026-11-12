//! Source-backed theme gallery and deterministic appearance commands (§14.7).
//!
//! The normal harness has no personal config and preserves earlier M6.3
//! references until a script selects a theme. Gallery commands then use the
//! same generated `Theme` constants as the live Session's ColorPolicy.
//! Each PNG comes from GPU readback; an RGB clear assertion and ink count
//! prevent a blank or stale frame from becoming a candidate reference.
//! Gallery images remain pending human review even when these checks pass.
//!
//! The older M6.3 frame references were captured before theme settings
//! existed. Those scenes create a bare VT and never opt into this module,
//! so their xterm-index colors retain the original expected pixels. The
//! gallery creates its own terminal and explicitly selects a built-in,
//! matching how the live window applies settings before the first frame.
//!
//! `appearance` is a simulated host fact. It does not read winit or the
//! current machine's light/dark preference, so a CI run gets the same
//! pair choice on macOS, Windows and Linux. `set theme.mode` can force a
//! side while preserving that simulated host fact; returning to `auto`
//! then chooses the most recently reported appearance.
//!
//! Scenario controls are transactional. A bad name or malformed mode is
//! rejected before it reaches the retained builder or VT state. A valid
//! switch discards OSC dynamic colors because they belong to the prior
//! appearance, then invalidates colored row instances. Text advances and
//! prepared glyph handles remain usable across palette changes.
//!
//! The gallery's clear sample is the first backing pixel, outside the
//! inset cell grid. The ink count is over GPU readback pixels, not VT
//! characters or authored foreground constants. It proves that a themed
//! frame was actually drawn while leaving visual quality to the user.
//! Golden comparisons run after the scenario's two independent assertions;
//! a blank newly blessed image cannot pass simply because a file exists.
use super::{Scenario, preview};
use puzzle_term::{
    settings::ThemeMode,
    theme::{Appearance, Theme},
};

/// Harness-only pair selection for `appearance` and `set theme.*` commands.
#[derive(Clone)]
pub(super) struct ThemeSelection {
    /// Last host preference supplied by a deterministic scenario command.
    system: Appearance,
    /// Host-following or forced side of the configured pair.
    mode: ThemeMode,
    /// Named light member, initially Puzzle Light.
    light: String,
    /// Named dark member, initially Puzzle Dark.
    dark: String,
    /// Resolved colors currently installed in the terminal builder.
    pub(super) active: Theme,
}

impl ThemeSelection {
    /// Starts with the same pair and dark fallback as product settings.
    fn initial() -> Result<Self, String> {
        // A missing generated default is a scenario error instead of a
        // silent fallback that would produce misleading gallery images.
        let active = Theme::builtin("puzzle-dark").ok_or("Puzzle Dark theme missing")?;
        Ok(Self {
            system: Appearance::Dark,
            mode: ThemeMode::Auto,
            light: "puzzle-light".into(),
            dark: "puzzle-dark".into(),
            active,
        })
    }

    /// Resolves the effective pair member after mode overrides the OS.
    fn resolve(&self) -> Result<Theme, String> {
        // Explicit light/dark choices are independent of the OS fact;
        // only Auto consults the last simulated appearance.
        let name = match self.mode {
            ThemeMode::Light => &self.light,
            ThemeMode::Dark => &self.dark,
            ThemeMode::Auto if self.system == Appearance::Light => &self.light,
            ThemeMode::Auto => &self.dark,
        };
        // A scenario may name only a reviewed built-in. Custom theme
        // validation is exercised by the neutral ThemeRegistry tests.
        Theme::builtin(name).ok_or_else(|| format!("unknown built-in theme: {name}"))
    }
}

/// Reports one host light/dark fact without consulting the real OS.
pub(super) fn appearance(state: &mut Scenario, raw: Option<&str>) -> Result<(), String> {
    // The two legal values are the same wire strings accepted by the
    // typed SetSystemAppearance decoder in puzzle-term.
    let appearance = match raw.ok_or("appearance needs light or dark")? {
        "light" => Appearance::Light,
        "dark" => Appearance::Dark,
        _ => return Err("appearance needs light or dark".into()),
    };
    let mut selection = state
        .theme_selection
        .clone()
        .map_or_else(ThemeSelection::initial, Ok)?;
    // Clone before mutation so a failed theme lookup leaves the prior
    // selection and its GPU colors available for later scenario commands.
    selection.system = appearance;
    let theme = selection.resolve()?;
    activate(state, &mut selection, theme)
}

/// Changes only the theme pair fields named by the §14.7 scenario grammar.
pub(super) fn set(state: &mut Scenario, line: &str) -> Result<(), String> {
    // Whitespace is the entire harness grammar. Quoted JSON belongs to
    // M6.5 config tests and is not accepted as a hidden extra argument.
    let mut words = line.split_whitespace();
    let _ = words.next();
    let key = words.next().ok_or("set needs a theme key")?;
    let value = words.next().ok_or("set needs a value")?;
    if words.next().is_some() {
        // Ignoring a trailing typo could leave the desired palette unset
        // while a later screenshot still paints from an earlier frame.
        return Err("set has extra fields".into());
    }
    let mut selection = state
        .theme_selection
        .clone()
        .map_or_else(ThemeSelection::initial, Ok)?;
    // Only theme pair fields are supported here. Broader settings patches
    // travel through SettingsStore in the separate M6.5 acceptance tests.
    match key {
        "theme.mode" => {
            selection.mode = match value {
                "auto" => ThemeMode::Auto,
                "light" => ThemeMode::Light,
                "dark" => ThemeMode::Dark,
                _ => return Err("invalid theme.mode".into()),
            }
        }
        "theme.light" => selection.light = value.into(),
        "theme.dark" => selection.dark = value.into(),
        _ => return Err(format!("unsupported set key: {key}")),
    }
    let theme = selection.resolve()?;
    // A dormant pair member can be named now and resolved on its next
    // selection; the active side must already have complete RGB tokens.
    activate(state, &mut selection, theme)
}

/// Installs colors and invalidates only the cached colored row geometry.
fn activate(
    state: &mut Scenario,
    selection: &mut ThemeSelection,
    theme: Theme,
) -> Result<(), String> {
    // Theme selection applies to visible terminal rows. An M0 clear-only
    // scenario has no VT grid and cannot assert colorized cell behavior.
    let terminal = state.terminal.as_mut().ok_or("theme needs terminal")?;
    if state
        .theme_selection
        .as_ref()
        .is_some_and(|old| old.active == theme)
    {
        // A redundant host callback should not erase OSC colors or force
        // another full row rebuild when the effective theme is unchanged.
        state.theme_selection = Some(selection.clone());
        return Ok(());
    }
    // One transaction clears every override and installs query/drawing defaults.
    theme.install_terminal_colors(&mut terminal.colors);
    terminal.appearance_dark = theme.appearance == Appearance::Dark;
    if let Some(builder) = state.frame_builder.as_mut() {
        // The builder may already hold several shaped rows; its epoch
        // invalidation recolors them without replacing the font context.
        builder.set_theme(&theme);
    }
    // The active Theme also drives the gallery's expected clear sample.
    selection.active = theme;
    state.theme_selection = Some(selection.clone());
    state.preview_ready = false;
    // Drawn-grid recognizer caches a prior readback and cannot answer
    // assertions about pixels after a theme switch.
    state.drawn_cache = None;
    Ok(())
}

/// Renders each generated built-in with byte-exact clear and visible ink.
pub(super) fn gallery(state: &mut Scenario) -> Result<(), String> {
    // The script supplies a styled sample once. Each loop iteration
    // exercises color resolution over the same retained VT cell tags.
    if state.terminal.is_none() {
        return Err("theme-gallery needs terminal".into());
    }
    for name in Theme::builtin_names() {
        // The generated source order is stable across runs and becomes
        // the numbered PNG order used by the golden comparator.
        let theme = Theme::builtin(name).ok_or("generated theme missing")?;
        let mut selection = state
            .theme_selection
            .clone()
            .map_or_else(ThemeSelection::initial, Ok)?;
        activate(state, &mut selection, theme.clone())?;
        // GPU submission and readback run after the palette transaction;
        // inspecting only the frame's CPU clear field would miss a renderer bug.
        preview::render(state)?;
        let image = state.image.as_ref().ok_or("gallery image missing")?;
        // The first pixel lies in the grid padding and must be the active
        // default background, including after a light-to-dark switch.
        if image.pixels.get(0..3) != Some(theme.background.as_slice()) {
            return Err(format!("{name}: GPU clear differs from theme background"));
        }
        let ink = image
            .pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[..3] != theme.background)
            .count();
        // One accidental cursor rectangle is not enough to call the sample
        // nonblank; at least a short text run must survive composition.
        if ink < 100 {
            return Err(format!("{name}: fewer than 100 non-background pixels"));
        }
        state.screenshot(name)?;
        // Screenshot numbering advances only after readback assertions,
        // preventing a failed palette from writing a candidate reference.
    }
    Ok(())
}
