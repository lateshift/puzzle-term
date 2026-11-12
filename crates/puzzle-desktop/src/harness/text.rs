//! M4 quoted text commands for the headless GPU harness (design §13.1).
//!
//! Scripts name cell or pixel positions and supply quoted UTF-8. This
//! adapter reads the approved font assets on the native platform, then
//! calls `puzzle-term::text_scene`, which shapes through puzzle-text and
//! prepares through the D-28 glyph backend seam. No grid model from M5
//! is introduced for these deterministic font specimens.
//!
//! `render-text` treats x/y as a physical-pixel top-left cell position.
//! `render-grid` uses explicit rows and columns solely to size a font
//! specimen target. It creates no cursor, VT parser or scrollback state.
//!
//! Every isolated screenshot reloads the three approved bundled files.
//! This keeps font bytes outside neutral crates and makes a missing asset
//! fail the script at the exact screenshot that needs it. Production text
//! will retain faces across frames; the harness deliberately tests the
//! first-use path from platform bytes through outline and atlas upload.
//!
//! Quoted UTF-8 stays intact until `unescape` has handled the small
//! scenario escape grammar. A `\\n` inside `render-grid` then becomes a
//! row separator; a plain space remains part of the shaping run. This
//! distinction matters for ligatures and for box glyphs joining across
//! adjacent cells. Style options live after the closing quote so a
//! literal word `bold` inside the text is never mistaken for a setting.
use puzzle_render::{Renderer, RgbaImage};
use puzzle_term::text_scene::{
    CellMetrics, CellOptions, FontData, FontSet, FontStyle, MetricAdjustment, TextLine, TextScene,
    cell_count,
};
use std::fs;

/// Script text retained until a `screenshot` consumes the request.
pub(super) enum TextRequest {
    /// One line beginning at a given device-pixel cell origin.
    Single {
        /// Left edge in physical device pixels.
        x: i32,
        /// Top edge in physical device pixels.
        y: i32,
        /// Logical pixels per em before DPR.
        size: f32,
        /// Quoted UTF-8 script content after escape decoding.
        text: String,
        /// Primary regular, bold or italic face selector.
        style: FontStyle,
        /// Whether coding substitutions may span neighbouring cells.
        ligatures: bool,
        /// Active scenario device-pixel ratio.
        dpr: f32,
    },
    /// Several fixed-width rows separated by escaped `\n` in the script.
    Grid {
        /// Number of terminal columns used to size the target.
        cols: u32,
        /// Number of terminal rows used to size the target.
        rows: u32,
        /// Quoted text with decoded line breaks between rows.
        text: String,
        /// Active scenario device-pixel ratio.
        dpr: f32,
    },
}

/// Parses the two M4 commands without confusing spaces inside quotes.
pub(super) fn parse(line: &str, dpr: f32) -> Result<TextRequest, String> {
    // Only one quoted field is accepted. Its first and last quote leave
    // position arguments outside the text, including text with spaces.
    let first = line.find('"').ok_or("text command needs a quoted string")?;
    let last = line
        .rfind('"')
        .ok_or("text command needs a closing quote")?;
    if last <= first {
        return Err("text command has an empty quote boundary".into());
    }
    // Decode before splitting rows, because a literal script backslash
    // must not be treated as a rendered newline by accident.
    let fields: Vec<_> = line[..first].split_whitespace().collect();
    let content = unescape(&line[first + 1..last])?;
    let suffix = line[last + 1..].trim();
    match fields.first().copied() {
        Some("render-text") if fields.len() == 4 => {
            // Absolute origins make the reference's expected pixels
            // independent of a font's negative side bearing.
            let x = fields[1].parse().map_err(|_| "render-text x is invalid")?;
            let y = fields[2].parse().map_err(|_| "render-text y is invalid")?;
            let size: f32 = fields[3].parse().map_err(|_| "render-text size is invalid")?;
            if !size.is_finite() || !(1.0..=256.0).contains(&size) { return Err("text size must be 1..=256".into()); }
            let (style, ligatures) = style_options(suffix)?;
            if content.contains('\n') {
                return Err("render-text accepts one line; use render-grid for rows".into());
            }
            // The style selects ink. Grid geometry still comes from the
            // same primary metrics regardless of substitutions.
            Ok(TextRequest::Single { x, y, size, text: content, style, ligatures, dpr })
        }
        Some("render-grid") if fields.len() == 3 => {
            // The fixed 14 px size in this command isolates joining and
            // fallback from user configuration that arrives in M6.
            if !suffix.is_empty() { return Err("render-grid has unexpected options".into()); }
            let cols: u32 = fields[1].parse().map_err(|_| "render-grid cols is invalid")?;
            let rows: u32 = fields[2].parse().map_err(|_| "render-grid rows is invalid")?;
            if cols == 0 || rows == 0 || cols > 512 || rows > 128 { return Err("render-grid dimensions are invalid".into()); }
            Ok(TextRequest::Grid { cols, rows, text: content, dpr })
        }
        _ => Err("usage: render-text X Y SIZE \"text\" [style=... ligatures=off] or render-grid COLS ROWS \"rows\"".into()),
    }
}

/// Turns script escapes into text while rejecting ambiguous malformed input.
fn unescape(input: &str) -> Result<String, String> {
    let mut output = String::new();
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        // Preserve Unicode scalars verbatim; only a backslash changes the
        // next character's interpretation in this small script language.
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        let escaped = match chars.next().ok_or("trailing text escape")? {
            // Unknown spellings must fail instead of dropping a slash
            // from a pasted sample and changing its shaped clusters.
            'n' => '\n',
            't' => '\t',
            '"' => '"',
            '\\' => '\\',
            other => return Err(format!("unsupported text escape: {other}")),
        };
        output.push(escaped);
    }
    // Shaping receives decoded UTF-8 rather than scenario syntax.
    Ok(output)
}

/// Applies only the small M4 style toggles needed by font specimens.
fn style_options(input: &str) -> Result<(FontStyle, bool), String> {
    // Coding ligatures default on (D-9); a specimen explicitly turns
    // them off to expose the difference under the same cell placement.
    let mut style = FontStyle::Regular;
    let mut ligatures = true;
    for token in input.split_whitespace() {
        // A typo is an error, since silently ignoring a toggle would
        // produce a plausible but misleading golden screenshot.
        match token {
            "style=regular" => style = FontStyle::Regular,
            "style=bold" => style = FontStyle::Bold,
            "style=italic" => style = FontStyle::Italic,
            "style=bold-italic" => style = FontStyle::BoldItalic,
            "ligatures=on" => ligatures = true,
            "ligatures=off" => ligatures = false,
            _ => return Err(format!("unsupported text option: {token}")),
        }
    }
    Ok((style, ligatures))
}

/// Reads platform assets, builds neutral placements and reads back GPU ink.
pub(super) fn render(renderer: &Renderer, request: &TextRequest) -> Result<RgbaImage, String> {
    // Platform code owns the filesystem. The core receives immutable
    // `FontData`, never an asset path or embedded font byte array.
    let regular = read_font("assets/fonts/JetBrainsMono-Variable.ttf")?;
    let italic = read_font("assets/fonts/JetBrainsMono-Italic-Variable.ttf")?;
    let symbols = read_font("assets/fonts/SymbolsNerdFontMono-Regular.ttf")?;
    let mut fonts = FontSet::new(regular, Some(italic), Some(symbols))?;
    // The text scene then applies shaping, placement and backend
    // preparation before this shared GPU readback runs.
    let (lines, width, height) = scene_lines(&fonts, request)?;
    // The scene builder never sees asset paths, only validated font data.
    let mut scene = TextScene::new(&mut fonts, &lines, width, height)?;
    renderer
        .render_slug_to_image(&mut scene.atlas, &scene.instances, width, height)
        .map_err(|error| error.to_string())
}

/// Sizes an isolated target and records the requested cell origins.
fn scene_lines<'a>(
    fonts: &FontSet,
    request: &'a TextRequest,
) -> Result<(Vec<TextLine<'a>>, u32, u32), String> {
    // Each borrowed line lives only until `TextScene::new` has shaped it.
    // No script text is retained in a GPU instance after scene creation.
    match request {
        TextRequest::Single { .. } => single_lines(fonts, request),
        TextRequest::Grid { .. } => grid_lines(fonts, request),
    }
}

/// Sizes a one-line specimen from primary metrics rather than ink bounds.
fn single_lines<'a>(
    fonts: &FontSet,
    request: &'a TextRequest,
) -> Result<(Vec<TextLine<'a>>, u32, u32), String> {
    let TextRequest::Single {
        x,
        y,
        size,
        text,
        style,
        ligatures,
        dpr,
    } = request
    else {
        return Err("expected a single-line request".into());
    };
    // Cell dimensions size the target; using font ink bounds
    // would make a bold or italic specimen's canvas change size.
    let cell = metrics(fonts, *size, *dpr)?;
    // The requested x/y name the cell corner, not the first
    // painted pixel; glyph side bearings remain observable.
    let line = TextLine {
        text,
        origin_px: [*x, *y],
        size: *size,
        dpr: *dpr,
        style: *style,
        ligatures: *ligatures,
        transformed: false,
    };
    // Unicode cell width, rather than scalar count, includes a wide
    // fallback glyph's second column in the review target.
    let count = cell_count(text)?;
    let (width, height) = (
        extent(*x, count.saturating_mul(cell.width_px))?,
        extent(*y, cell.height_px)?,
    );
    Ok((vec![line], width, height))
}

/// Sizes a row specimen with integer cell origins for joining checks.
fn grid_lines<'a>(
    fonts: &FontSet,
    request: &'a TextRequest,
) -> Result<(Vec<TextLine<'a>>, u32, u32), String> {
    let TextRequest::Grid {
        cols,
        rows,
        text,
        dpr,
    } = request
    else {
        return Err("expected a grid request".into());
    };
    let mut lines = Vec::new();
    // All rows share one 14 px primary-face geometry so a box
    // continuation meets its neighbour at an identical x edge.
    let cell = metrics(fonts, 14.0, *dpr)?;
    // Split only after escape decoding, so `\\n` in the script
    // becomes the row delimiter expected by `render-grid`.
    let content: Vec<_> = text.split('\n').collect();
    // More supplied lines than declared rows would silently clip
    // evidence; reject them before preparing any GPU resources.
    if content.len() > *rows as usize {
        return Err("grid has more text rows than declared".into());
    }
    for (row, line) in content.iter().enumerate() {
        // A declared grid narrower than its source must fail instead of
        // silently clipping the right edge of fallback or box glyphs.
        if cell_count(line)? > *cols {
            return Err(format!("grid row {row} exceeds {cols} columns"));
        }
        // Sixteen physical pixels of margin show any overshoot
        // from a font outline or Slug's half-pixel dilation.
        let y = 16 + i32::try_from(row as u32 * cell.height_px).map_err(|_| "grid y overflow")?;
        lines.push(TextLine {
            text: line,
            origin_px: [16, y],
            size: 14.0,
            dpr: *dpr,
            style: FontStyle::Regular,
            ligatures: true,
            transformed: false,
        });
    }
    // Blank declared rows remain part of the target but have no
    // glyph instances, letting `expect-pixel` check background.
    let (width, height) = (
        32 + cols.saturating_mul(cell.width_px),
        32 + rows.saturating_mul(cell.height_px),
    );
    Ok((lines, width, height))
}

/// Loads one approved bundled face without embedding it in a neutral crate.
pub(super) fn read_font(path: &str) -> Result<FontData, String> {
    // Include the path in a failure report so an asset packaging mistake
    // identifies the missing file rather than a later tofu appearance.
    // These are repository assets, just as font_data's bundled faces are;
    // Cargo runs unit tests from the crate directory rather than the workspace.
    let absolute = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path);
    fs::read(absolute)
        .map(|bytes| FontData::new(bytes, 0))
        .map_err(|error| format!("{path}: {error}"))
}

/// Derives cell dimensions from the same face and D-27 policy as layout.
pub(super) fn metrics(fonts: &FontSet, size: f32, dpr: f32) -> Result<CellMetrics, String> {
    // A Symbols Nerd Font fallback may have very different advances.
    // Primary metrics keep its icon from moving later terminal cells.
    let face = fonts
        .face(fonts.primary_face(FontStyle::Regular))
        .ok_or("primary face missing")?;
    CellMetrics::from_face(
        face,
        CellOptions {
            size,
            dpr,
            line_height: 1.0,
            width: MetricAdjustment::Px(0.0),
            height: MetricAdjustment::Px(0.0),
            baseline: MetricAdjustment::Px(0.0),
            snap_cap_height: true,
            // Light hinting already aligns caps below DPR 1.5. Higher
            // DPR uses original outlines and may use bounded cap snapping.
            hinted: dpr < 1.5,
        },
    )
}

/// Adds a review margin and rejects negative or unbounded target positions.
fn extent(origin: i32, content: u32) -> Result<u32, String> {
    // Negative script origins are rejected because the reviewed image
    // must contain the complete first cell, including its left bearing.
    u32::try_from(origin)
        .map_err(|_| "text origin must be nonnegative")?
        .checked_add(content)
        .and_then(|value| value.checked_add(16))
        // This matches the renderer's maximum texture size for scenes;
        // rejecting early avoids a device validation error or panic.
        .filter(|value| *value <= 8192)
        .ok_or("text target exceeds 8192 device pixels".into())
}
