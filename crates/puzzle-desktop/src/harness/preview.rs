//! Headless terminal bytes and images through the M6 frame contract (§13.1).
//!
//! The harness feeds captured PTY bytes into the pure VT core, then draws its
//! visible rows through the persistent `FrameBuilder` and `FrameRenderer`.
//! Colors are resolved into GPU instances before drawing; screenshot readback
//! is only evidence output and never feeds a second color-compositing pass.
//! Each fixture is replayed from source bytes so parser state, alternate
//! screen switching and styled cells all affect the evidence image.
//!
//! A scenario creates its terminal explicitly before a recording is fed.
//! This avoids accidentally appending a second capture to an old screen,
//! which could make both parser assertions and visual evidence misleading.
//! The grid limits are narrower than the engine's general security limits
//! because a preview also creates an offscreen GPU target for every cell.
//!
//! The `feed` grammar is intentionally small: it is for deterministic
//! escape bytes in a scenario, while large real streams come from `replay`.
//! The assertion reads cells before GPU rendering, so text correctness and
//! raster correctness remain distinguishable when one of them breaks.
//!
//! The platform shell reads fonts from files; neither the VT core nor the
//! composition crate embeds them or depends on a filesystem. The same
//! neutral frame is consumed by the eventual interactive winit surface.
//! A dump is taken from VT cells before drawing; it distinguishes a parsing
//! error from a raster error when the screenshot looks unexpectedly empty.
//!
//! Cursor-setting scenarios keep one builder alive across screenshots.
//! Recreating it would let a broken style-epoch invalidation pass because
//! every cache would start cold. The fixed command grammar represents the
//! four cursor fields until M6.5 supplies the full settings schema.
use super::{Scenario, shell, text};
use puzzle_render::FrameRenderer;
use puzzle_term::{
    frame_builder::FrameBuilder,
    frame_builder::context::TextContext,
    session::cursor_settings::{CursorBlink, CursorColor, CursorSettings, CursorStyle},
    text_scene::FontSet,
    vt::{Terminal, terminal::ActiveScreen},
};
use std::fs;

/// Converts visible VT rows into M4 text requests and a reviewable grid dump.
mod grid;

/// Records VT cells, or the dimensions of an M0 clear-only scenario.
pub(super) fn dump(state: &mut Scenario, raw: Option<&str>) -> Result<(), String> {
    if state.shell.is_some() {
        return shell::dump(state, raw);
    }
    // A clear-only script has no VT cells, but dimensions distinguish a
    // valid blank target from a screenshot made at an unintended size.
    let label = raw.ok_or("dump needs a label")?;
    let path = state.path(label, "grid.txt");
    let grid = state.terminal.as_ref().map_or_else(
        || format!("M0 empty grid: {}x{} cells\n", state.cols, state.rows),
        grid_dump,
    );
    fs::write(path, grid).map_err(|error| error.to_string())
}

/// Adds a partial-cell strip to a terminal screenshot's backing store.
pub(super) fn set_surface_extra(state: &mut Scenario, raw: Option<&str>) -> Result<(), String> {
    let value = raw.ok_or("surface-extra needs WxH device pixels")?;
    let (width, height) = value
        .split_once('x')
        .ok_or("surface-extra needs WxH device pixels")?;
    // The renderer validates these against device-cell metrics when it
    // has a font. The script may set them before creating a terminal.
    state.surface_extra = [
        width.parse().map_err(|_| "invalid extra width")?,
        height.parse().map_err(|_| "invalid extra height")?,
    ];
    Ok(())
}

/// Creates a bounded terminal with enough history for captured programs.
pub(super) fn terminal(state: &mut Scenario, size: Option<&str>) -> Result<(), String> {
    // Explicit dimensions keep the PTY capture and its screenshot in the
    // same coordinate system; `size` remains M0's placeholder clear command.
    let (cols, rows) = grid_size(size)?;
    // Captures need little history, so the preview caps retained output
    // below the production default while still exercising scrollback.
    state.terminal = Some(Terminal::new(cols, rows, 1_000, 4 * 1024 * 1024));
    // A fresh screen must not be paired with a prior fixture's image.
    state.preview_ready = false;
    state.image = None;
    state.drawn_cache = None;
    state.frame_builder = None;
    state.text_context = None;
    state.cursor_settings = CursorSettings::default();
    state.theme_selection = None;
    state.frame_renderer = None;
    state.old_row_versions.clear();
    Ok(())
}

/// Sets the four cursor fields for a deterministic appearance scenario.
pub(super) fn cursor_settings(state: &mut Scenario, line: &str) -> Result<(), String> {
    // A missing field must be a scenario error, not an inherited value
    // from an earlier screenshot with unrelated appearance preferences.
    let mut words = line.split_whitespace();
    let _ = words.next();
    // User-facing shape names differ from DECSCUSR codes: the latter stay
    // stored in VT even while these configured shapes override drawing.
    let style = match words.next().ok_or("cursor-settings needs style")? {
        "block" => CursorStyle::Block,
        "bar" => CursorStyle::Bar,
        "underline" => CursorStyle::Underline,
        "block_hollow" => CursorStyle::BlockHollow,
        _ => return Err("unknown cursor style".into()),
    };
    // Automatic blink still consults VT mode 12 and the app's steady code;
    // forced on/off must remain distinguishable in the script.
    let blink = match words.next().ok_or("cursor-settings needs blink")? {
        "auto" => CursorBlink::Auto,
        "on" => CursorBlink::On,
        "off" => CursorBlink::Off,
        _ => return Err("unknown cursor blink".into()),
    };
    // The ignored application style remains queryable through DECRQSS,
    // which is why this permission is separate from the shape field.
    let allow_app_style = match words.next().ok_or("cursor-settings needs allow-app")? {
        "true" => true,
        "false" => false,
        _ => return Err("allow-app must be true or false".into()),
    };
    // RGB syntax is validated before it can enter a packed GPU instance.
    let color = CursorColor::parse(words.next().ok_or("cursor-settings needs color")?)?;
    // Rejecting extra words catches a typo in a future scenario edit.
    if words.next().is_some() {
        return Err("cursor-settings has extra fields".into());
    }
    let settings = CursorSettings {
        style,
        blink,
        allow_app_style,
        color,
    };
    // Store preferences before an optional first font load. Subsequent
    // renders apply the same setting to the retained, warm builder.
    state.cursor_settings = settings;
    if let Some(builder) = state.frame_builder.as_mut() {
        // A setting change must affect an already warmed frame builder;
        // recreating it would hide a stale-cache regression.
        builder.set_cursor_settings(settings);
    }
    // A screenshot can only save the next render-terminal result; the old
    // image cannot satisfy the new setting's pixel assertions.
    state.preview_ready = false;
    Ok(())
}

/// Feeds one privacy-reviewed `script` recording as raw PTY bytes.
pub(super) fn replay(state: &mut Scenario, path: Option<&str>) -> Result<(), String> {
    let path = path.ok_or("replay needs a fixture path")?;
    // The harness opens fixture files; puzzle-vt itself has no I/O (§4.1).
    let bytes = fs::read(path).map_err(|error| format!("{path}: {error}"))?;
    // The harness-only fault injection needs the exact pre-read revisions;
    // recording them after parsing would make a skipped damage undetectable.
    super::drawn::remember_versions(state)?;
    // Feed raw bytes, including OSC and CSI delimiters, so replay tests the
    // same parser callbacks that a live PTY will use in M6.
    state
        .terminal
        .as_mut()
        .ok_or("replay needs terminal")?
        .advance(&bytes);
    // The explicit diagnostic grant adds a real Session/Surface witness beside
    // this unchanged raw VT replay, never derives authority from fixture bytes.
    super::extra_replay(state, path, &bytes)?;
    // Any subsequent screenshot now requires a new render-terminal step.
    state.preview_ready = false;
    state.drawn_cache = None;
    Ok(())
}

/// Feeds a short quoted sequence for scenario-specific cursor or mode checks.
pub(super) fn feed(state: &mut Scenario, line: &str) -> Result<(), String> {
    // Allocate once for the decoded command; this path is scenario control,
    // outside puzzle-vt's measured per-byte steady-state print loop.
    let bytes = feed_bytes(line)?;
    // The terminal owns the bounded vte parser across feed calls, which
    // makes split escape sequences observable in the next command.
    // A short `feed` has the same revision boundary as a captured PTY read,
    // so the stale-cache check works for both scripted and recorded bytes.
    super::drawn::remember_versions(state)?;
    state
        .terminal
        .as_mut()
        .ok_or("feed needs terminal")?
        .advance(&bytes);
    state.preview_ready = false;
    state.drawn_cache = None;
    Ok(())
}

/// Asserts a visible row prefix before drawing can conceal parser mistakes.
pub(super) fn expect_text(state: &Scenario, line: &str) -> Result<(), String> {
    // Prefix matching ignores unspecified trailing blanks and applications'
    // varying status details while pinning a few known leading cells.
    let (head, _) = line.split_once('"').ok_or("expect-text needs quotes")?;
    let row: usize = head
        .split_whitespace()
        .nth(1)
        .ok_or("missing row")?
        .parse()
        .map_err(|_| "invalid row")?;
    let wanted = quoted(line, "expect-text")?;
    let term = state
        .terminal
        .as_ref()
        .ok_or("expect-text needs terminal")?;
    // row_text skips wide spacers and expands stored grapheme clusters; an
    // assertion thus names what a user sees rather than storage sentinels.
    let actual = grid::row_text(term, row)?;
    if actual.starts_with(wanted) {
        Ok(())
    } else {
        Err(format!(
            "row {row} was {actual:?}, wanted prefix {wanted:?}"
        ))
    }
}

/// Confirms that a full-screen fixture switched the active VT buffer.
pub(super) fn expect_screen(state: &Scenario, name: Option<&str>) -> Result<(), String> {
    let term = state
        .terminal
        .as_ref()
        .ok_or("expect-screen needs terminal")?;
    let wanted = match name.ok_or("expect-screen needs primary or alternate")? {
        "primary" => ActiveScreen::Primary,
        "alternate" => ActiveScreen::Alternate,
        _ => return Err("expect-screen needs primary or alternate".into()),
    };
    // Vim's 1049 enter and leave must affect the active grid; a screenshot
    // of stale primary text could otherwise satisfy an ink-only assertion.
    if term.active == wanted {
        Ok(())
    } else {
        Err(format!("screen was {:?}, wanted {:?}", term.active, wanted))
    }
}

/// Returns a compact visible grid for the reflow and preview review trail.
pub(super) fn grid_dump(term: &Terminal) -> String {
    // This compact text dump is review evidence, not a snapshot of every
    // style or parser field; large opaque snapshots are forbidden by §2.7.
    (0..term.size().1)
        .filter_map(|row| grid::row_text(term, row).ok())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Renders visible cells through the same neutral frame as the live shell.
pub(super) fn render(state: &mut Scenario) -> Result<(), String> {
    let term = state
        .terminal
        .as_ref()
        .ok_or("render-terminal needs terminal")?;
    if state.frame_builder.is_none() {
        // Load only once per terminal scenario. Reusing the builder retains
        // shaped runs and Slug handles across multiple screenshots (P4).
        let regular = text::read_font("assets/fonts/JetBrainsMono-Variable.ttf")?;
        let italic = text::read_font("assets/fonts/JetBrainsMono-Italic-Variable.ttf")?;
        let symbols = text::read_font("assets/fonts/SymbolsNerdFontMono-Regular.ttf")?;
        let fonts = FontSet::new(regular, Some(italic), Some(symbols))?;
        let context = TextContext::new(fonts, 14.0, state.dpr)?;
        let mut builder = FrameBuilder::new(&context);
        builder.set_cursor_settings(state.cursor_settings);
        // Gallery scripts select a source-backed theme before the first
        // frame, whereas older M6.3 scenes keep their reviewed baseline.
        if let Some(selection) = &state.theme_selection {
            builder.set_theme(&selection.active);
        }
        state.frame_builder = Some(builder);
        state.text_context = Some(context);
    }
    // The renderer retains atlas planes between frames; rebuilding it for
    // each screenshot would lose prepared glyphs with no dirty upload.
    let renderer = state.frame_renderer.get_or_insert_with(|| {
        FrameRenderer::new(&state.renderer, wgpu::TextureFormat::Rgba8Unorm)
    });
    let builder = state
        .frame_builder
        .as_mut()
        .ok_or("terminal fonts missing")?;
    let metrics = builder.metrics();
    let (cols, rows) = term.size();
    // A live surface may leave fewer than one device cell at its right or
    // bottom edge. Exercise that area in a deterministic screenshot too.
    if state.surface_extra[0] >= metrics.width_px || state.surface_extra[1] >= metrics.height_px {
        return Err("surface-extra must be smaller than one device cell".into());
    }
    // Checked arithmetic turns an oversized scenario into a script error;
    // a wrapped extent could make the GPU screenshot assert the wrong edge.
    let width = u32::try_from(cols)
        .ok()
        .and_then(|value| value.checked_mul(metrics.width_px))
        .and_then(|value| value.checked_add(32))
        .and_then(|value| value.checked_add(state.surface_extra[0]))
        .ok_or("terminal surface width exceeds GPU limit")?;
    let height = u32::try_from(rows)
        .ok()
        .and_then(|value| value.checked_mul(metrics.height_px))
        .and_then(|value| value.checked_add(32))
        .and_then(|value| value.checked_add(state.surface_extra[1]))
        .ok_or("terminal surface height exceeds GPU limit")?;
    builder.set_surface_size([width, height]);
    let context = state
        .text_context
        .as_mut()
        .ok_or("terminal text context missing")?;
    builder.build(context, term, &mut state.terminal_frame)?;
    let (image, _) = renderer
        .render_to_image(&state.renderer, &mut state.terminal_frame)
        .map_err(|error| error.to_string())?;
    state.image = Some(image);
    // The attachment has changed; a candidate sheet from the old pixels
    // cannot answer a later drawn-grid assertion for this frame.
    state.drawn_cache = None;
    state.preview_ready = true;
    state.slug_scene = None;
    state.text_request = None;
    Ok(())
}

/// Extracts exactly one quoted argument without accepting trailing fields.
pub(super) fn quoted<'a>(line: &'a str, command: &str) -> Result<&'a str, String> {
    // A single quoted field is enough for feed and expect-text. Rejecting
    // trailing tokens avoids accidentally ignoring a second assertion.
    let (head, rest) = line.split_once('"').ok_or("missing opening quote")?;
    if !head.trim_start().starts_with(command) {
        return Err("wrong quoted command".into());
    }
    let (value, tail) = rest.split_once('"').ok_or("missing closing quote")?;
    if !tail.trim().is_empty() {
        return Err("unexpected text after quote".into());
    }
    Ok(value)
}

/// Decodes authored feed escapes once for VT previews and Session scenarios.
pub(super) fn feed_bytes(line: &str) -> Result<Vec<u8>, String> {
    let raw = quoted(line, "feed")?;
    let mut bytes = Vec::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            // UTF-8 passes through unchanged, so test scenes can feed wide
            // scalars without translating them through a shell locale.
            let mut buffer = [0; 4];
            bytes.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
            continue;
        }
        // The small grammar names control bytes without a shell escaping
        // layer, so a typo cannot silently change a VT fixture.
        let byte = match chars.next().ok_or("trailing feed escape")? {
            'e' => 0x1b,
            'n' => b'\n',
            'r' => b'\r',
            't' => b'\t',
            '\\' => b'\\',
            '"' => b'"',
            other => return Err(format!("bad feed escape {other}")),
        };
        bytes.push(byte);
    }
    Ok(bytes)
}

/// Validates dimensions before either creating or resizing an offscreen grid.
fn grid_size(size: Option<&str>) -> Result<(usize, usize), String> {
    let size = size.ok_or("terminal needs COLSxROWS")?;
    let (cols, rows) = size.split_once('x').ok_or("terminal needs COLSxROWS")?;
    let cols: usize = cols.parse().map_err(|_| "invalid terminal columns")?;
    let rows: usize = rows.parse().map_err(|_| "invalid terminal rows")?;
    // Limit preview targets before VT allocation and GPU texture creation.
    if !(2..=512).contains(&cols) || !(1..=128).contains(&rows) {
        return Err("preview terminal dimensions out of range".into());
    }
    Ok((cols, rows))
}

/// Applies real VT reflow while retaining the warmed builder and glyph context.
pub(super) fn resize(state: &mut Scenario, raw: Option<&str>) -> Result<(), String> {
    let (cols, rows) = grid_size(raw)?;
    // Preserve the old revisions for incremental-versus-cold frame comparisons.
    super::drawn::remember_versions(state)?;
    state
        .terminal
        .as_mut()
        .ok_or("resize-terminal needs terminal")?
        .resize(cols, rows);
    // Pixels and recognition from the old geometry cannot satisfy new assertions.
    state.preview_ready = false;
    state.image = None;
    state.drawn_cache = None;
    Ok(())
}
