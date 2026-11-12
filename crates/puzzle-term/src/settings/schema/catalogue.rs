//! Complete v1 key catalogue in the `define_settings!` DSL (§14.5).
//!
//! Rich maps remain typed as the neutral Value tree until their owning
//! subsystem validates entries. Scalar fields use concrete Rust types;
//! the generator emits the same default and key for code and docs.
use super::{BlinkMode, ShiftCapture, ThemeMode, Validator, define_settings};
use crate::settings::value::SettingValue;
use puzzle_bus::Value;
use std::collections::BTreeMap;

define_settings! {
    /// Automatic native shell snippets; Apple bash 3.2 retains manual sourcing and normal login behavior.
    shell_integration: String = String::from("detect") => "shellIntegration", Validator::Choice(&["detect", "none", "bash", "zsh", "fish"]), NextSession, Desktop;

    /// Primary monospace family.
    font_family: String = String::from("JetBrains Mono") => "font.family", Validator::Any, Live, All;
    /// Bundled symbol fallback family.
    font_symbols: String = String::from("Symbols Nerd Font Mono") => "font.symbols", Validator::Any, Live, All;
    /// Glyph backend selector.
    font_renderer: String = String::from("auto") => "font.renderer", Validator::Choice(&["auto", "slug", "atlas"]), Live, All;
    /// Bitmap glyph source when an atlas backend is enabled.
    font_rasterizer: String = String::from("oracle") => "font.rasterizer", Validator::Choice(&["oracle", "browser", "platform"]), Live, All;
    /// Largest device size selected for an atlas glyph.
    font_atlas_max_px: u32 = 16 => "font.atlasMaxPx", Validator::Range { minimum: 1.0, maximum: 256.0 }, Live, All;
    /// Outline fitting policy at small device sizes.
    font_hinting: String = String::from("auto") => "font.hinting", Validator::Choice(&["auto", "none", "light", "native"]), Live, All;
    /// Text compositing transfer function.
    font_blending: String = String::from("native") => "font.blending", Validator::Choice(&["native", "linear"]), Live, All;
    /// Optional optical weight on Slug coverage.
    font_thicken: bool = false => "font.thicken", Validator::Any, Live, All;
    /// Font size in logical pixels.
    font_size: f32 = 14.0 => "font.size", Validator::Range { minimum: 4.0, maximum: 200.0 }, Live, All;
    /// Multiplier for line height beyond face metrics.
    font_line_height: f32 = 1.2 => "font.lineHeight", Validator::Range { minimum: 0.5, maximum: 3.0 }, Live, All;
    /// Enable coding ligatures in shaped runs.
    font_ligatures: bool = true => "font.ligatures", Validator::Any, Live, All;
    /// OpenType feature tags applied during shaping.
    font_features: Vec<String> = Vec::new() => "font.features", Validator::Any, Live, All;
    /// Ordered additional font family fallbacks.
    font_fallback: Vec<String> = Vec::new() => "font.fallback", Validator::Any, Live, All;
    /// Generate box, block and Braille glyphs from device cells.
    font_builtin_glyphs: bool = true => "font.builtinGlyphs", Validator::Any, Live, All;
    /// Snap unhinted capitals within the allowed size nudge.
    font_snap_cap_height: bool = true => "font.snapCapHeight", Validator::Any, Live, All;
    /// Permit a symbol to enter a following empty cell.
    font_symbol_overflow: bool = true => "font.symbolOverflow", Validator::Any, Live, All;
    /// Break a ligature at the editing cursor.
    font_ligatures_break_at_cursor: bool = true => "font.ligaturesBreakAtCursor", Validator::Any, Live, All;
    /// Device-independent adjustment to cell width.
    font_cell_width: f32 = 0.0 => "font.cellWidth", Validator::Any, Live, All;
    /// Device-independent adjustment to cell height.
    font_cell_height: f32 = 0.0 => "font.cellHeight", Validator::Any, Live, All;
    /// Offset the font baseline within its cell.
    font_baseline_offset: f32 = 0.0 => "font.baselineOffset", Validator::Any, Live, All;

    /// Choose host-following or forced appearance.
    theme_mode: ThemeMode = ThemeMode::Auto => "theme.mode", Validator::Choice(&["auto", "light", "dark"]), Live, All;
    /// Built-in or custom theme for light appearance.
    theme_light: String = String::from("puzzle-light") => "theme.light", Validator::Any, Live, All;
    /// Built-in or custom theme for dark appearance.
    theme_dark: String = String::from("puzzle-dark") => "theme.dark", Validator::Any, Live, All;
    /// Crossfade duration for a theme change.
    theme_transition_ms: u32 = 0 => "theme.transitionMs", Validator::Any, Live, All;
    /// Minimum foreground/background contrast at render time.
    theme_minimum_contrast: f32 = 1.0 => "theme.minimumContrast", Validator::Range { minimum: 1.0, maximum: 21.0 }, Live, All;
    /// Opacity of the terminal background surface.
    theme_background_opacity: f32 = 1.0 => "theme.backgroundOpacity", Validator::Range { minimum: 0.0, maximum: 1.0 }, Live, All;
    /// Map bold base ANSI colors to bright ANSI colors.
    theme_bold_is_bright: bool = false => "theme.boldIsBright", Validator::Any, Live, All;
    /// Expose resolved theme tokens as CSS variables.
    theme_export_css_variables: bool = true => "theme.exportCssVariables", Validator::Any, Live, Web;
    /// Custom named theme specifications.
    themes: Value = Value::Map(BTreeMap::new()) => "themes", Validator::Any, Live, All;
    /// Named profile override maps.
    profiles: Value = Value::Map(BTreeMap::new()) => "profiles", Validator::Any, Live, All;
    /// Active profile name, empty for none.
    profile: String = String::new() => "profile", Validator::Any, Live, All;

    /// Default cursor geometry when the application has no override.
    cursor_style: String = String::from("block") => "cursor.style", Validator::Choice(&["block", "bar", "underline", "block_hollow"]), Live, All;
    /// Defer to the application or force cursor blinking on or off.
    cursor_blink: BlinkMode = BlinkMode::Auto => "cursor.blink", Validator::Any, Live, All;
    /// Seconds of inactivity before cursor blink stops.
    cursor_blink_timeout_s: u32 = 15 => "cursor.blinkTimeoutS", Validator::Any, Live, All;
    /// Bar and underline cursor thickness in logical pixels.
    cursor_thickness: f32 = 1.0 => "cursor.thickness", Validator::Range { minimum: 0.5, maximum: 16.0 }, Live, All;
    /// Animate cursor position between cells.
    cursor_animate: bool = false => "cursor.animate", Validator::Any, Live, All;
    /// Ignore an application's cursor shape without losing its DECRQSS answer.
    cursor_allow_app_style: bool = true => "cursor.allowAppStyle", Validator::Any, Live, All;
    /// Choose the theme cursor, inverse cell colors or a fixed hex color.
    cursor_color: String = String::from("theme") => "cursor.color", Validator::CursorColor, Live, All;
    /// Per-session image allowance in MiB, accounted separately for decoded and GPU storage; each image is charged max(decoded RGBA, tiled GPU bytes including borders); zero disables images.
    images_storage_limit_mb: u32 = 320 => "images.storageLimitMb", Validator::Any, Live, All;
    /// Maximum retained primary-screen history rows.
    scrollback_lines: u32 = 10_000 => "scrollback.lines", Validator::Range { minimum: 0.0, maximum: 1_000_000.0 }, Live, All;
    /// Maximum retained primary-screen history bytes.
    scrollback_max_bytes: u32 = 32 * 1024 * 1024 => "scrollback.maxBytes", Validator::Any, Live, All;

    /// Wheel movement multiplier in text rows.
    scrolling_multiplier: f32 = 3.0 => "scrolling.multiplier", Validator::Any, Live, All;
    /// Interpolate viewport movement between rows.
    scrolling_smooth: bool = true => "scrolling.smooth", Validator::Any, Live, All;
    /// Settle fractional scrolling on row boundaries.
    scrolling_snap_to_rows: bool = true => "scrolling.snapToRows", Validator::Any, Live, All;
    /// Return to the newest row when the user types.
    scrolling_on_input: bool = true => "scrolling.onInput", Validator::Any, Live, All;
    /// Return to the newest row when output arrives.
    scrolling_on_output: bool = false => "scrolling.onOutput", Validator::Any, Live, All;
    /// Characters that delimit selection words.
    selection_word_separators: String = String::from(" `│|:\"'()[]{}<>,;") => "selection.wordSeparators", Validator::Any, Live, All;
    /// Copy selected text when a drag completes.
    selection_copy_on_select: bool = false => "selection.copyOnSelect", Validator::Any, Live, All;
    /// Snap simple selection endpoints in default trailing blanks to the line end.
    selection_line_end_snap: bool = true => "selection.lineEndSnap", Validator::Any, Live, All;
    /// Remove default trailing blanks from copied lines.
    selection_trim_trailing: bool = true => "selection.trimTrailing", Validator::Any, Live, All;

    /// Treat Option as Alt according to platform policy.
    keyboard_option_as_alt: String = String::from("none") => "keyboard.optionAsAlt", Validator::Choice(&["none", "left", "right", "both"]), Live, All;
    /// Choose DEL or BS for the Backspace key.
    keyboard_backspace: String = String::from("del") => "keyboard.backspace", Validator::Choice(&["del", "bs"]), Live, All;
    /// Layered keybinding declarations.
    keyboard_bindings: Vec<String> = Vec::new() => "keyboard.bindings", Validator::Any, Live, All;
    /// Start with an empty binding table.
    keyboard_clear_defaults: bool = false => "keyboard.clearDefaults", Validator::Any, Live, All;
    /// Timeout for unfinished key sequences in milliseconds.
    keyboard_sequence_timeout_ms: u32 = 0 => "keyboard.sequenceTimeoutMs", Validator::Any, Live, All;
    /// Shift mouse capture: false/true are program-overridable defaults; always/never are fixed. Legacy mouse.shiftSelects and mouse_shift_selects map true to never, false to always. Shift-wheel stays local (§7.4).
    mouse_shift_capture: ShiftCapture = ShiftCapture::False => "mouse.shiftCapture", Validator::Any, Live, All;
    /// Hide the pointer temporarily while typing.
    mouse_hide_while_typing: bool = true => "mouse.hideWhileTyping", Validator::Any, Live, All;
    /// Convert wheel events into keys on the alternate screen.
    mouse_alternate_scroll: bool = true => "mouse.alternateScroll", Validator::Any, Live, All;
    /// Policy for terminal-requested clipboard writes.
    clipboard_osc52_write: String = String::from("allow") => "clipboard.osc52Write", Validator::Choice(&["allow", "deny"]), Live, All;
    /// Policy for terminal-requested clipboard reads.
    clipboard_osc52_read: String = String::from("deny") => "clipboard.osc52Read", Validator::Choice(&["allow", "deny"]), Live, All;
    /// Recognize plain-text URLs under the pointer.
    links_detect_urls: bool = true => "links.detectUrls", Validator::Any, Live, All;
    /// Schemes allowed when opening a detected link.
    links_schemes: Vec<String> = vec!["http".into(), "https".into(), "mailto".into()] => "links.schemes", Validator::Any, Live, All;
    /// User gesture required to open a hyperlink.
    links_open: String = String::from("modifierClick") => "links.open", Validator::Choice(&["modifierClick", "disabled"]), Live, All;
    /// Flash the terminal when BEL arrives.
    bell_visual: bool = true => "bell.visual", Validator::Any, Live, All;
    /// Play a sound when BEL arrives.
    bell_sound: bool = false => "bell.sound", Validator::Any, Live, All;
    /// Cell width used for ambiguous Unicode characters.
    unicode_ambiguous_width: u32 = 1 => "unicode.ambiguousWidth", Validator::Range { minimum: 1.0, maximum: 2.0 }, Live, All;
    /// Choose grapheme clustering or legacy per-scalar cell widths.
    unicode_grapheme_width: String = String::from("unicode") => "unicode.graphemeWidth", Validator::Choice(&["unicode", "legacy"]), Live, All;

    /// Keep or close a normally exited child session.
    session_on_exit: String = String::from("close") => "session.onExit", Validator::Choice(&["close", "hold"]), Live, Desktop;
    /// Hold even with close when an early abnormal exit is shorter than this.
    session_abnormal_exit_ms: u32 = 250 => "session.abnormalExitMs", Validator::Range { minimum: 0.0, maximum: 60_000.0 }, Live, All;

    /// Inset between the terminal grid and window edge.
    render_padding: f32 = 4.0 => "render.padding", Validator::Any, Live, All;
    /// GPU power preference for a newly created device.
    render_power: String = String::from("low") => "render.power", Validator::Choice(&["low", "high"]), NextSession, All;
    /// Composite text and backgrounds in linear light.
    render_linear_blending: bool = false => "render.linearBlending", Validator::Any, Live, All;
    /// Maximum wait before presenting synchronized output.
    render_sync_timeout_ms: u32 = 200 => "render.syncTimeoutMs", Validator::Any, Live, All;
    /// Coalesce a short output burst before its next refresh presentation.
    render_settle_ms: u32 = 8 => "render.settleMs", Validator::Range { minimum: 0.0, maximum: 16.0 }, Live, All;
    /// Memory in MiB for each glyph atlas separately (vector and raster); minimum is one mask page plus one RGBA page (or the vector page minimum, if larger).
    render_glyph_memory_mb: u32 = 48 => "render.glyphMemoryMb", Validator::Range { minimum: crate::frame_builder::context::budget::MIN_MIB as f64, maximum: u32::MAX as f64 }, Live, All;
    /// Fill padded edges with the default background.
    render_extend_edge_background: bool = false => "render.extendEdgeBackground", Validator::Any, Live, All;
    /// Expose a screen-reader oriented text view.
    accessibility_screen_reader: bool = false => "accessibility.screenReader", Validator::Any, Live, All;
    /// Maximum parser time allowed for one output feed call.
    performance_parse_budget_ms: u32 = 8 => "performance.parseBudgetMs", Validator::Any, Live, All;
    /// Structured tracing filter specification.
    logging_filter: String = String::from("info") => "logging.filter", Validator::Any, Live, Desktop;
    /// Optional native log path; an empty value disables the file sink.
    logging_file: String = String::new() => "logging.file", Validator::Any, Live, Desktop;
}
