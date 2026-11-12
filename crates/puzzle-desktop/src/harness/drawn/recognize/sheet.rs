//! Same-pipeline raster candidate sheets and normalized coverage scores.
//!
//! A sheet places candidates in separated cells so shaping cannot create
//! ligatures across candidates. Each sheet is specific to the source cell's
//! colors and attributes. The screenshot and sheet are rendered through
//! identical fonts, DPR, hinting and GPU shaders, which makes a pixel score
//! meaningful even when an indexed palette or inverse video is active.
//!
//! A terminal cell is not a bitmap. Its stored scalar and style are inputs
//! to shaping, glyph selection, hinting, Slug curve preparation, and final
//! compositing. Reading only `Cell.ch` would test the parser, not the image.
//! The candidate sheet drives those same stages and compares their output
//! to bytes copied back from the actual GPU attachment.
//!
//! Pixel positions use device units. FrameBuilder places the grid after a
//! sixteen-pixel border, while `CellMetrics` supplies the size of each cell
//! after DPR scaling. The atlas uses the same border and metrics. A template
//! crop can therefore be compared to an application cell without resizing,
//! which would erase exactly the one-pixel defects this tool is meant to find.
//!
//! Coverage is inferred from three RGB samples at each pixel: the observed
//! color, a blank candidate, and a filled-block candidate. The latter two
//! establish a local color axis. This matters for reverse video, dim ink,
//! and palette entries: a global black/white threshold would call colored
//! glyphs background or turn inverse cells into solid blocks.
//!
//! The full block need not be mathematically opaque at every boundary pixel.
//! The same rasterizer draws it and every candidate, so its antialiased edge
//! is a consistent normalization reference. Where the two references are
//! identical, no color evidence exists and the projected coverage is zero.
//! That case is intentionally treated as ambiguous rather than inventing
//! hidden text from the VT character stored behind identical ink and paper.

use crate::harness::text;
use puzzle_render::{FrameRenderer, Renderer, RgbaImage};
use puzzle_term::{
    frame::RenderFrame,
    frame_builder::FrameBuilder,
    text::font_set::FontSet,
    vt::{Cell, CellFlags, Terminal},
};

/// Candidate coverage arrays for one color and attribute combination.
pub(super) struct Sheet {
    /// Text represented by the matching array at the same index.
    pub(super) chars: Vec<char>,
    /// Coverage masks packed in row-major device pixels.
    pub(super) masks: Vec<Vec<u8>>,
    /// Per-pixel blank color used to normalize the scenario cell.
    pub(super) blank: Vec<[u8; 3]>,
    /// Per-pixel full-block color supplies the coverage direction.
    pub(super) block: Vec<[u8; 3]>,
}

/// Draws all candidates with the observed cell style in one GPU submission.
///
/// Each sheet belongs to one style key. Reusing a sheet across truecolor
/// backgrounds would corrupt coverage normalization even when the glyph's
/// geometry happened to be identical. Sheet construction is diagnostic work
/// and is never part of live frame assembly or the P1 allocation budget.
pub(super) fn make_sheet(
    renderer: &Renderer,
    font_size: f32,
    dpr: f32,
    cell_size: (u32, u32),
    source: &Terminal,
    cell: &Cell,
    chars: &[char],
) -> Result<Sheet, String> {
    // The measured cell dimensions preserve fractional-DPR rounding from
    // the same font metrics used for the observed frame.
    let (width, height) = cell_size;
    let term = sheet_terminal(source, cell, chars)?;
    // Native file reads mirror the harness's platform font supply. The
    // neutral text crate does not embed these bytes (§8.1, P7).
    let fonts = FontSet::new(
        text::read_font("assets/fonts/JetBrainsMono-Variable.ttf")?,
        Some(text::read_font(
            "assets/fonts/JetBrainsMono-Italic-Variable.ttf",
        )?),
        Some(text::read_font(
            "assets/fonts/SymbolsNerdFontMono-Regular.ttf",
        )?),
    )?;
    let mut context = puzzle_term::frame_builder::context::TextContext::new(fonts, font_size, dpr)?;
    let mut builder = FrameBuilder::new(&context);
    // An empty builder avoids old row-cache entries from the scenario;
    // otherwise a stale atlas could validate its own stale output.
    let mut frame = RenderFrame::default();
    builder.build(&mut context, &term, &mut frame)?;
    let mut gpu = FrameRenderer::new(renderer, wgpu::TextureFormat::Rgba8Unorm);
    let (image, _) = gpu
        .render_to_image(renderer, &mut frame)
        .map_err(|error| error.to_string())?;
    // Atlas slot i begins at column 2*i within one 32-slot row. Its next
    // column remains blank, so an `=` cannot shape together with a following
    // `>` and accidentally become a ligature in an isolated candidate.
    let mut pixels = Vec::with_capacity(chars.len());
    for index in 0..chars.len() {
        pixels.push(crop(
            &image,
            16 + ((index % 32) * 2) as u32 * width,
            16 + (index / 32) as u32 * height,
            width,
            height,
        )?);
    }
    let blank = pixels.first().ok_or("empty sheet")?.clone();
    let block = pixels.get(1).ok_or("sheet lacks full block")?.clone();
    // The first two candidates are fixed sentinels, so every style uses
    // precisely the same per-pixel background and solid-ink references.
    let masks = pixels
        .iter()
        .map(|rgb| coverage(rgb, &blank, &block))
        .collect();
    Ok(Sheet {
        chars: chars.to_vec(),
        masks,
        blank,
        block,
    })
}

/// Copies palette policy and lays candidates into isolated two-cell slots.
///
/// Terminal::new supplies ordinary default state, but OSC 4, OSC 10/11 and
/// reverse video can alter the source's resolved colors without modifying
/// its cells. The synthetic terminal copies those settings before rendering.
/// Its cursor is hidden because the atlas tests glyph ink, not a cursor at
/// the first candidate's location; cursor context is handled separately.
fn sheet_terminal(source: &Terminal, cell: &Cell, chars: &[char]) -> Result<Terminal, String> {
    // Thirty-two candidate slots per row keep the GPU target under its
    // maximum texture width even at 2× DPR, while a second column prevents
    // shaping across candidate boundaries.
    let mut term = Terminal::new(64, chars.len().div_ceil(32), 0, 0);
    term.modes.set(true, 25, false);
    term.modes
        .set(true, 5, source.modes.get(true, 5) == Some(true));
    term.colors = source.colors;
    // A candidate inherits style but not the source's width or sparse-extra
    // flags. Those flags describe the source row's storage, not the new
    // scalar placed into this independent cell.
    for (index, ch) in chars.iter().enumerate() {
        let mut glyph = *cell;
        glyph.ch = u32::from(*ch);
        glyph.flags = CellFlags::empty();
        // The freshly allocated sheet has no old extras. The host edit seam
        // stamps this diagnostic write with the same clock as parser output.
        term.edit_row(index / 32, |row| row.cells[(index % 32) * 2] = glyph);
    }
    Ok(term)
}

/// Copies one physical cell, rejecting a backing extent mismatch.
///
/// The readback format has tightly packed RGBA bytes; the fourth channel
/// carries opacity, but coverage comparison uses encoded RGB because the
/// final attachment has already composited every glyph over its background.
/// Rejecting an out-of-bounds crop keeps a wrong DPR or stale surface size
/// from masquerading as a blank or partially read cell.
pub(super) fn crop(
    image: &RgbaImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<Vec<[u8; 3]>, String> {
    // A subtraction-based bound check can underflow on hostile dimensions;
    // these coordinates come from bounded scenarios, and addition is small
    // compared with the GPU's texture limit checked before rendering.
    if x + width > image.width || y + height > image.height {
        return Err("drawn cell exceeds backing image".into());
    }
    let mut rgb = Vec::with_capacity((width * height) as usize);
    for dy in 0..height {
        for dx in 0..width {
            // Row-major indexing follows RgbaImage's own unpadded layout;
            // the GPU's 256-byte readback padding was stripped earlier.
            let index = ((y + dy) * image.width + x + dx) as usize * 4;
            rgb.push([
                image.pixels[index],
                image.pixels[index + 1],
                image.pixels[index + 2],
            ]);
        }
    }
    Ok(rgb)
}

/// Projects encoded colors onto the blank-to-ink direction at each pixel.
///
/// Let B be the blank pixel, F the filled-block pixel and P the observed
/// pixel. The least-squares coverage on the RGB segment is
/// `dot(P-B,F-B)/dot(F-B,F-B)`. Dotting all channels avoids choosing red as
/// a special case, which fails on cyan or blue text. The score is clamped
/// before quantization because decorations and cursor overlays may extend
/// outside that color segment.
pub(super) fn coverage(rgb: &[[u8; 3]], blank: &[[u8; 3]], block: &[[u8; 3]]) -> Vec<u8> {
    rgb.iter()
        .zip(blank)
        .zip(block)
        .map(|((pixel, bg), fg)| {
            // This vector can point toward darker values under inverse
            // video; its squared length is positive in either direction.
            let delta = [
                f32::from(fg[0]) - f32::from(bg[0]),
                f32::from(fg[1]) - f32::from(bg[1]),
                f32::from(fg[2]) - f32::from(bg[2]),
            ];
            let norm = delta.iter().map(|value| value * value).sum::<f32>();
            if norm < 1.0 {
                // Identical ink and paper cannot reveal a glyph. Returning
                // zero lets the ambiguity rule decide rather than dividing
                // by zero or amplifying one-byte color noise.
                return 0;
            }
            let dot = (0..3)
                .map(|i| (f32::from(pixel[i]) - f32::from(bg[i])) * delta[i])
                .sum::<f32>();
            // Byte coverage is enough: both candidate and observed masks
            // are derived from eight-bit attachment samples.
            (dot / norm * 255.0).clamp(0.0, 255.0).round() as u8
        })
        .collect()
}

/// Chooses the closest visible candidate and marks genuine ambiguity.
///
/// An L1 distance across all device pixels resists isolated antialiasing
/// differences while retaining the geometry of thin box strokes. Scores
/// are normalized by cell area before the confidence rule, so a 2× cell
/// cannot pass merely because it contains four times as many pixels.
/// A contextual template is admitted only by the caller for ligatures,
/// wide glyphs and cursor overlays; ordinary ASCII recognition remains
/// independent of the current grid label.
pub(super) fn classify(
    sheet: &Sheet,
    observed: &[u8],
    context_char: char,
    context: &[u8],
    allow_context: bool,
) -> (char, Option<String>) {
    // Candidate order is deterministic for equal scores, but a near tie
    // above the noise floor is reported as '?' rather than guessing.
    let mut ranked: Vec<(u32, usize)> = sheet
        .masks
        .iter()
        .enumerate()
        .map(|(index, mask)| {
            let error = observed
                .iter()
                .zip(mask)
                .map(|(left, right)| left.abs_diff(*right) as u32)
                .sum();
            (error, index)
        })
        .collect();
    ranked.sort_unstable();
    let (best, index) = ranked[0];
    let second = ranked.get(1).map_or(u32::MAX, |entry| entry.0);
    let contextual = observed
        .iter()
        .zip(context)
        .map(|(left, right)| left.abs_diff(*right) as u32)
        .sum::<u32>();
    // Whole-row shaping changes ligature pixels, so isolated single-char
    // templates cannot be exact for those cells. A contextual candidate
    // must still match the readback more closely than the isolated set.
    let count = observed.len().max(1) as u32;
    if allow_context && contextual < best {
        return (context_char, None);
    }
    // On K3 self-test cells at DPR 1 hinted / DPR 2, the largest correct
    // per-pixel error was 1.665 / 0.223 bytes. The closest distinguishable
    // wrong candidate and narrowest correct margin were 4.684 / 3.015 bytes.
    // A two-byte error ceiling lies between those populations; the one-byte
    // tie margin is below the narrowest measured margin by at least 3x.
    // Exact duplicate masks tie at zero error, so the margin rule starts
    // above one byte instead of rejecting indistinguishable blank cells.
    if best > count * 2 || (best > count && second.saturating_sub(best) < count) {
        // Notes preserve both scores and the tempting best label so an
        // implementer can see whether the problem is missing candidates,
        // damaged pixels or genuinely indistinguishable small glyphs.
        return (
            '?',
            Some(format!(
                "best={:.2} second={:.2} candidate={:?}",
                best as f32 / count as f32,
                second as f32 / count as f32,
                sheet.chars[index]
            )),
        );
    }
    (sheet.chars[index], None)
}
