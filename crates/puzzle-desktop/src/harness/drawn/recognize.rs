//! Identifies cell ink by comparing readback with same-pipeline glyph templates.
//!
//! A template sheet spaces glyphs by one blank cell so Harfrust cannot join
//! neighboring candidates into ligatures. Sheets preserve the source cell's
//! attributes and tagged colors; the same FrameBuilder, fonts, hinting mode,
//! GPU shaders and DPR draw both the sheet and the scenario image. Per-pixel
//! color projection then compares coverage rather than RGB values, which
//! keeps palette and inverse-video cells in a common score range.
//!
//! The grid contributes a candidate alphabet and the style to draw that
//! alphabet, but never supplies the answer for ordinary cells. A stale
//! incremental row can therefore be read back as its old characters even
//! though the VT cells now contain new ones. That is the failure K3 must
//! distinguish from a parser error or an incorrectly encoded key.
//!
//! Some pixels are inherently contextual. JetBrains Mono can join `->`
//! into one two-cell outline; a wide glyph covers its spacer; the cursor
//! may invert a cell after text rendering. For those cases only, a fresh
//! full-row frame supplies one additional candidate. It is compared against
//! the actual readback, not accepted solely because the grid names it.
//! Ordinary ASCII and box drawing still compete with the isolated atlas.
//!
//! This remains a diagnostic rather than a general OCR engine. Identical
//! glyphs at tiny sizes cannot be distinguished from pixels alone, and a
//! mismatch is reported with two scores rather than invented as a letter.

use super::Scenario;
use puzzle_render::{Renderer, RgbaImage};
use puzzle_term::vt::{Cell, CellFlags, Terminal};
use std::collections::{BTreeSet, HashMap};

/// Failure-only complete candidate masks and scores survive scenario retries.
pub(in crate::harness) mod failure;

/// Appearance tags determine which candidate sheet can serve a cell.
type Style = (u32, u32, u16);

/// One recognized physical row per string, plus uncertain-cell scores.
pub(crate) struct Readback {
    /// Pixel-inferred text in physical cell order.
    pub(crate) rows: Vec<String>,
    /// Ambiguous and low-scoring cells remain inspectable.
    pub(in crate::harness) notes: Vec<String>,
    /// Exact masks and scores from rejected cells, never regenerated on retry.
    failures: Vec<serde_json::Value>,
}

impl Readback {
    /// Gives `dump-drawn` the same newline convention as a grid dump.
    pub(in crate::harness) fn text(&self) -> String {
        self.rows.join("\n") + "\n"
    }
}

/// Reuses one pixel recognition across assertions on the same image.
pub(crate) fn cached(state: &mut Scenario) -> Result<&Readback, String> {
    if state.drawn_cache.is_none() {
        // A cell sheet can require many glyph renders. Repeating it for
        // absent, grid and row-change assertions on identical pixels adds
        // cost without producing independent evidence.
        state.drawn_cache = Some(read(state)?);
    }
    state
        .drawn_cache
        .as_ref()
        .ok_or("drawn cache missing after recognition".into())
}

/// Requires a shell row's rendered glyphs to change after a key command.
pub(super) fn expect_row_changed(state: &Scenario, raw: Option<&str>) -> Result<(), String> {
    let row: usize = raw
        .ok_or("expect-drawn-row-changed needs row")?
        .parse()
        .map_err(|_| "invalid drawn row")?;
    let before = state
        .prior_drawn_rows
        .as_ref()
        .and_then(|rows| rows.get(row))
        .ok_or("no preceding drawn row")?;
    // A grid-only comparison could pass despite a stale row cache, so
    // both values come from the same pixel recognizer used by K3.
    let current = read(state)?;
    let after = current.rows.get(row).ok_or("drawn row outside grid")?;
    if before == after {
        return Err(format!("drawn row {row} did not change"));
    }
    Ok(())
}

/// Requires at least one row's pixels to respond to a non-scroll command.
pub(super) fn expect_any_row_changed(state: &Scenario) -> Result<(), String> {
    let before = state
        .prior_drawn_rows
        .as_ref()
        .ok_or("no preceding drawn frame")?;
    let after = read(state)?;
    // Ctrl+A/X edit a number under nvim's cursor, which may sit below the
    // top row; a top-row assertion would confuse that valid edit with no-op.
    if before.iter().zip(&after.rows).any(|(old, new)| old != new) {
        Ok(())
    } else {
        Err("no drawn row changed after key".into())
    }
}

/// Renders and scores isolated candidates for one observed style.
mod sheet;
use sheet::{Sheet, classify, coverage, crop, make_sheet};

/// Keeps one recognition pass's immutable images and reusable style sheets.
struct ReadContext<'a> {
    /// Device shared by observed pixels and freshly rendered candidates.
    renderer: &'a Renderer,
    /// Logical text size used for this frame.
    font_size: f32,
    /// Device scale selects hinting and whole-pixel metrics.
    dpr: f32,
    /// VT supplies candidate styles, never the claimed rendered result.
    term: &'a Terminal,
    /// Incremental pixels are the evidence under test.
    image: &'a RgbaImage,
    /// A cold rendering supplies contextual ligatures and cursor templates.
    cold: RgbaImage,
    /// Device-pixel dimensions identify exact cell crops.
    cell_size: (u32, u32),
    /// Every printable candidate considered for each style.
    chars: Vec<char>,
    /// One rendered candidate sheet per observed appearance combination.
    sheets: HashMap<Style, Sheet>,
    /// Scores for cells that cannot be identified with confidence.
    notes: Vec<String>,
    /// Failed-cell evidence uses the same GPU sheet as the classification.
    failures: Vec<serde_json::Value>,
}

/// Reads a rendered terminal image without trusting the grid's glyph labels.
///
/// The cold image is built once per assertion. Building it inside the cell
/// loop would multiply GPU submissions by grid area and could sample a
/// different blink phase for different cells. The parser and input adapter
/// are not touched during this pass: readback observes one frozen state.
pub(crate) fn read(state: &Scenario) -> Result<Readback, String> {
    let term = state
        .terminal
        .as_ref()
        .ok_or("drawn readback needs terminal")?;
    let image = state
        .image
        .as_ref()
        .ok_or("drawn readback needs rendered frame")?;
    // A full-row contextual template retains ligatures such as `->` and
    // cursor inversion. Isolated candidate cells alone cannot reproduce
    // either; the contextual candidate still has to match the readback.
    let cold = super::cold_image(state)?;
    let metrics = state
        .frame_builder
        .as_ref()
        .ok_or("drawn readback needs metrics")?
        .metrics();
    read_from(
        &state.renderer,
        term,
        image,
        cold,
        (metrics.width_px, metrics.height_px),
        14.0,
        state.dpr,
    )
}

/// Recognizes a frozen frame captured outside the general scenario runner.
pub(crate) fn read_frozen(
    renderer: &Renderer,
    term: &Terminal,
    warm: &RgbaImage,
    cold: RgbaImage,
    cell_size: (u32, u32),
    font_size: f32,
    dpr: f32,
) -> Result<(String, Vec<String>), String> {
    // Action replay already owns the snapshot and GPU readback. Moving it
    // into Scenario would lose the recorded parse and presentation boundary.
    let readback = read_from(renderer, term, warm, cold, cell_size, font_size, dpr)?;
    Ok((readback.text(), readback.notes))
}

/// Scores the rendered pixels with their exact font and device-cell metrics.
pub(super) fn read_from(
    renderer: &Renderer,
    term: &Terminal,
    image: &RgbaImage,
    cold: RgbaImage,
    cell_size: (u32, u32),
    font_size: f32,
    dpr: f32,
) -> Result<Readback, String> {
    // Cache sheets by the stored color tags and attributes, including
    // underline style. A colored chart can have many keys; a plain editor
    // typically reuses one or two sheets across thousands of cells.
    let mut context = ReadContext {
        renderer,
        font_size,
        dpr,
        term,
        image,
        cold,
        cell_size,
        chars: candidates(term),
        sheets: HashMap::new(),
        notes: Vec::new(),
        failures: Vec::new(),
    };
    let mut rows = Vec::with_capacity(term.size().1);
    // Preserve physical cell order rather than Unicode scalar count.
    // This lets a reported `row:column` point to the same region as a
    // screenshot and the terminal's cursor coordinates.
    for row_index in 0..term.size().1 {
        rows.push(context.read_row(row_index)?);
    }
    Ok(Readback {
        rows,
        notes: context.notes,
        failures: context.failures,
    })
}

impl ReadContext<'_> {
    /// Matches each physical cell of one visible row in left-to-right order.
    ///
    /// A wide spacer has no separate glyph. It occupies a real physical
    /// column, so the output keeps one blank placeholder to maintain cell
    /// coordinates even though a copied text row would omit it.
    fn read_row(&mut self, row_index: usize) -> Result<String, String> {
        let row = self
            .term
            .screen()
            .row(row_index)
            .ok_or("visible row missing")?;
        let mut drawn = String::new();
        for (column, cell) in row.cells.iter().enumerate() {
            if cell.flags.contains(CellFlags::WIDE_SPACER) {
                drawn.push(' ');
                continue;
            }
            let (ch, note) = self.read_cell(row_index, column, cell)?;
            // Notes use the physical column while the uncertainty is still
            // local; no later Unicode string index can shift it.
            drawn.push(ch);
            if let Some(note) = note {
                self.notes.push(format!("{row_index}:{column} {note}"));
            }
        }
        Ok(drawn)
    }

    /// Compares readback coverage with isolated and contextual candidates.
    ///
    /// A sheet is first prepared on demand for the cell's appearance. Then
    /// both the incremental and cold images are cropped at the same device
    /// coordinates. Normalization uses one color axis from that sheet, so
    /// neither image gets an advantage from having different RGB math.
    fn read_cell(
        &mut self,
        row: usize,
        column: usize,
        cell: &Cell,
    ) -> Result<(char, Option<String>), String> {
        let style = (cell.fg.0, cell.bg.0, cell.attrs.bits());
        if let std::collections::hash_map::Entry::Vacant(entry) = self.sheets.entry(style) {
            // Entry avoids rendering the same font/style atlas again when
            // many adjacent cells share SGR state.
            entry.insert(make_sheet(
                self.renderer,
                self.font_size,
                self.dpr,
                self.cell_size,
                self.term,
                cell,
                &self.chars,
            )?);
        }
        let sheet = self.sheets.get(&style).ok_or("candidate sheet missing")?;
        let (width, height) = self.cell_size;
        // FrameBuilder's grid starts at (16,16) after the overhang border;
        // the cells themselves tile in exact device-pixel increments.
        let x = 16 + column as u32 * width;
        let y = 16 + row as u32 * height;
        let pixels = crop(self.image, x, y, width, height)?;
        let contextual = crop(&self.cold, x, y, width, height)?;
        let observed = coverage(&pixels, &sheet.blank, &sheet.block);
        let expected = coverage(&contextual, &sheet.blank, &sheet.block);
        // A zero scalar is the terminal's unwritten blank, not U+0000 ink.
        // Cluster sentinels cannot be converted into one scalar here and
        // intentionally remain uncertain until cluster templates are added.
        let ch = if cell.ch == 0 {
            ' '
        } else {
            char::from_u32(cell.ch).unwrap_or('?')
        };
        let allow_context = cell.flags.contains(CellFlags::WIDE)
            // Cursor presentation is an overlay and can invert or fill the
            // cell after the isolated glyph shape is drawn.
            || (self.term.modes.get(true, 25) == Some(true)
                && row == self.term.cursor.row
                && column == self.term.cursor.column)
            || has_ligature_neighbor(self.term, row, column);
        // The classifier may choose an atlas glyph instead of the context
        // glyph; that is how a stale row becomes visible as old text.
        let result = classify(sheet, &observed, ch, &expected, allow_context);
        if result.0 != ch || result.1.is_some() {
            // Preserve the actual comparison, not a new GPU render after failure.
            self.failures.push(failure::cell(
                sheet,
                &pixels,
                &observed,
                (row, column),
                self.cell_size,
            ));
        }
        Ok(result)
    }
}

/// Limits contextual matching to shapes that an isolated glyph cannot draw.
///
/// These operators are the coding-ligature sequences in the current probe
/// fixtures. Looking two columns back catches both halves of a three-cell
/// sequence. The exact source row is still not trusted as an answer: its
/// contextual pixels must outscore independently rendered characters.
fn has_ligature_neighbor(term: &Terminal, row: usize, column: usize) -> bool {
    let Some(cells) = term.screen().row(row).map(|line| &line.cells) else {
        return false;
    };
    for left in column.saturating_sub(2)..=column {
        // The terminal holds scalar cells, while Harfrust may combine a
        // short run when preparing a whole row for rendering.
        let pair: String = cells
            .iter()
            .skip(left)
            .take(3)
            .filter_map(|cell| char::from_u32(cell.ch))
            .collect();
        if ["->", "=>", "!=", "==", "<=", ">=", "-->", "===", ".."]
            .iter()
            .any(|token| pair.starts_with(token) && column < left + token.len())
        {
            return true;
        }
    }
    false
}

/// Collects printable ASCII, drawing symbols and real screen characters.
///
/// Space and the full block occupy fixed slots for the normalization axis.
/// ASCII and Unicode drawing ranges cover the expected terminal repertoire;
/// characters actually present in this screen extend it for local symbols.
/// A sorted set keeps candidate order deterministic, including ties.
fn candidates(term: &Terminal) -> Vec<char> {
    let mut extra = BTreeSet::new();
    for row_index in 0..term.size().1 {
        // Gathering symbols does not imply they are the answer. They must
        // later compete by coverage score with every other candidate.
        if let Some(row) = term.screen().row(row_index) {
            for cell in &row.cells {
                if let Some(ch) = char::from_u32(cell.ch) {
                    extra.insert(ch);
                }
            }
        }
    }
    let mut chars = vec![' ', '█'];
    for point in (0x21..=0x7e).chain(0x2500..=0x257f).chain(0x2580..=0x259f) {
        if let Some(ch) = char::from_u32(point) {
            extra.insert(ch);
        }
    }
    chars.extend(extra.into_iter().filter(|ch| *ch != ' ' && *ch != '█'));
    chars
}
