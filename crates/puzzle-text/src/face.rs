//! Validated OpenType face access for M4 text (design §8.1, §8.3, §8.5).
//!
//! The platform owns the font bytes. A face stores the chosen variation
//! location and harfrust's owned shaping data, then reopens a lightweight
//! `FontRef` when a table is needed. This avoids a self-referential borrow
//! while keeping malformed external font data behind `Result` reports.
//!
//! Hinted outlines use skrifa's ppem-specific engine and return to em
//! coordinates before Slug preparation. Design advances never come from
//! those shifted outline points, preserving grid placement (D-27, D-28).
//!
//! Skrifa and harfrust share the same `read-fonts` version (§2.6), so the
//! `FontRef` passed to either engine has one table representation. The
//! face keeps both engines' variation positions at the same `wght` value;
//! otherwise a bold glyph could shape with one width and draw another.
//! An optional face with invalid tables is rejected before it enters a
//! fallback chain. Glyph IDs from shaping are checked against glyph_count
//! before any outline lookup, so hostile font data returns an error.
use crate::{FontData, glyph::HintMode, probe::EmPen};
use harfrust::{ShaperData, ShaperInstance};
use kurbo::BezPath;
use rustc_hash::FxHashMap;
use skrifa::{
    FontRef, GlyphId, MetadataProvider,
    instance::{Location, Size},
    outline::{DrawSettings, Engine, HintingInstance, HintingOptions, SmoothMode, Target},
};

/// One validated font face and its size-specific hinter cache.
pub struct FontFace {
    /// Immutable platform-supplied OpenType bytes and collection index.
    data: FontData,
    /// The same explicit weight lets a worker build an equivalent owned face once.
    weight: u16,
    /// Skrifa normalized coordinates used by outlines and metrics.
    location: Location,
    /// Harfrust table and plan data prepared once for repeated runs.
    shaper_data: ShaperData,
    /// Harfrust coordinates matching the same variable weight.
    shaper_instance: ShaperInstance,
    /// Reused auto or native hinting engines, keyed by ppem and mode.
    hinters: FxHashMap<(u16, HintMode), HintingInstance>,
    /// Validated units per em, needed to normalize outline callbacks.
    upem: u16,
    /// Number of glyph IDs accepted from a shaped run.
    glyph_count: u16,
    /// True when the face can express bold weight without synthetic ink.
    variable_weight: bool,
    /// Whether GSUB advertises any coding-ligature feature for ASCII.
    ascii_ligatures: bool,
    /// Ordinary text bypasses color-table parsing in the steady-state row path.
    has_colr: bool,
    /// Printable ASCII IDs resolved once, avoiding a font-table parse per cell.
    ascii_glyphs: [Option<u32>; 128],
    /// Non-ASCII coverage reuses the same cmap selection loaded with this face.
    charmap: skrifa::charmap::MappingIndex,
}

/// Parses font bytes and binds the variation location to one style face.
impl FontFace {
    /// Rejects a malformed collection face or an invalid units-per-em value.
    ///
    /// `FontRef::from_index` validates the table directory but loads tables
    /// lazily. The explicit metric check prevents a later divide by zero
    /// when the pen converts font units to em-space Slug coordinates.
    pub fn new(data: FontData, weight: u16) -> Result<Self, String> {
        let font = FontRef::from_index(data.bytes(), data.index())
            .map_err(|error| format!("invalid OpenType face: {error}"))?;
        let variable_weight = font.axes().get_by_tag(skrifa::Tag::new(b"wght")).is_some();
        // One validated scan at face load lets the per-run ASCII decision
        // avoid reparsing GSUB while still using the exact feature rule.
        let ascii_ligatures = gsub_has_ascii_ligatures(&font)?;
        // A terminal row repeatedly uses the same small alphabet. Keeping
        // its cmap answers avoids reopening OpenType tables for every cell.
        let ascii_glyphs = std::array::from_fn(|byte| {
            char::from_u32(byte as u32).and_then(|ch| font.charmap().map(ch).map(GlyphId::to_u32))
        });
        // Both engines receive the same user-space weight. A face without
        // that axis simply stays at its default location.
        let location = font.axes().location([("wght", f32::from(weight))]);
        let shaper_instance = ShaperInstance::from_variations(
            &font,
            [(harfrust::Tag::new(b"wght"), f32::from(weight))],
        );
        let metrics = font.metrics(Size::unscaled(), &location);
        // A face with zero UPEM has no meaningful scale, and zero glyphs
        // cannot satisfy even the terminal's ASCII fallback needs.
        if metrics.units_per_em == 0 || metrics.glyph_count == 0 {
            return Err("font has no usable glyph units or glyphs".into());
        }
        // ShaperData owns parsed layout caches and can outlive this short
        // borrow of the underlying font bytes.
        let shaper_data = ShaperData::new(&font);
        let has_colr = font.table_data(skrifa::Tag::new(b"COLR")).is_some();
        let charmap = skrifa::charmap::MappingIndex::new(&font);
        Ok(Self {
            data,
            weight,
            location,
            shaper_data,
            shaper_instance,
            hinters: FxHashMap::default(),
            upem: metrics.units_per_em,
            glyph_count: metrics.glyph_count,
            variable_weight,
            ascii_ligatures,
            has_colr,
            ascii_glyphs,
            charmap,
        })
    }

    /// Reopens the borrowed table view without copying the owned bytes.
    ///
    /// A stored `FontRef` would borrow `self.data` from within the same
    /// struct. Reopening its directory avoids a self-referential owner.
    pub fn font(&self) -> Result<FontRef<'_>, String> {
        FontRef::from_index(self.data.bytes(), self.data.index())
            .map_err(|error| format!("font tables became unavailable: {error}"))
    }

    /// Shares the existing immutable bytes without copying the loaded font.
    pub fn data(&self) -> FontData {
        self.data.clone()
    }
    /// Preserves the chosen variation instance across the host preparation seam.
    pub fn weight(&self) -> u16 {
        self.weight
    }
    /// A first hinter construction has unpredictable cost and must run off-frame.
    pub fn hint_ready(&self, ppem: u16, mode: HintMode) -> bool {
        ppem == 0 || mode == HintMode::None || self.hinters.contains_key(&(ppem, mode))
    }

    /// Skips COLR lookup entirely for the normal monochrome text path.
    pub(crate) fn has_colr(&self) -> bool {
        self.has_colr
    }

    /// Borrows the chosen variation coordinates for COLR graph traversal.
    pub(crate) fn location(&self) -> &Location {
        &self.location
    }

    /// Names the shared bytes and collection index for once-per-font diagnostics.
    pub(crate) fn identity(&self) -> (usize, u32) {
        (self.data.bytes().as_ptr() as usize, self.data.index())
    }

    /// Finds a scalar's real glyph ID, leaving missing-glyph policy above.
    ///
    /// The charmap returns `None` for absent characters. Substituting the
    /// font's glyph zero here would prevent the fallback chain from trying
    /// Symbols Nerd Font or the builtin last-resort box.
    pub fn glyph_id(&self, ch: char) -> Result<Option<u32>, String> {
        if ch.is_ascii() {
            return Ok(self.ascii_glyphs[ch as usize]);
        }
        Ok(self
            .charmap
            .charmap(&self.font()?)
            .map(ch)
            .map(GlyphId::to_u32))
    }

    /// Reads the prevalidated ASCII cmap without opening font tables.
    pub fn ascii_glyph(&self, byte: u8) -> Option<u32> {
        self.ascii_glyphs[usize::from(byte)]
    }

    /// Reports the design-unit advance, independent of outline hinting.
    ///
    /// Harfrust's shaped advances are ignored for terminal grid placement;
    /// this metric only informs cell width and fallback fitting.
    pub fn advance_em(&self, glyph_id: u32) -> Result<f32, String> {
        let font = self.font()?;
        let id = self.checked_id(glyph_id)?;
        let advance = font
            .glyph_metrics(Size::unscaled(), &self.location)
            .advance_width(id)
            .ok_or("font has no glyph advance")?;
        Ok(advance / f32::from(self.upem))
    }

    /// Returns the parsed face's shaping tables for a temporary shaper.
    /// A run can reuse these tables without reparsing GSUB/GPOS.
    pub fn shaper_data(&self) -> &ShaperData {
        &self.shaper_data
    }

    /// Returns the variation coordinates paired with shaping tables.
    /// The outline location above is set from the same `wght` value.
    pub fn shaper_instance(&self) -> &ShaperInstance {
        &self.shaper_instance
    }

    /// Measures unscaled metrics at the face's variable-font location.
    /// The cell geometry layer applies the display DPR after normalization.
    pub fn metrics(&self) -> Result<skrifa::metrics::Metrics, String> {
        Ok(self.font()?.metrics(Size::unscaled(), &self.location))
    }

    /// Reports whether this face lacks a real bold-weight variation.
    /// The caller applies this only in a bold style, including fallbacks.
    pub fn needs_synthetic_bold(&self) -> bool {
        !self.variable_weight
    }

    /// Reports whether liga, calt or dlig can substitute printable ASCII.
    pub fn has_ascii_ligatures(&self) -> bool {
        self.ascii_ligatures
    }

    /// Draws one validated glyph, optionally fitting points at one ppem.
    ///
    /// The unhinted path uses design units and is valid at every size.
    /// The hinted path uses a cached ppem-specific engine and divides its
    /// pixel-space callbacks back to em coordinates for the same packer.
    pub fn outline(&mut self, glyph_id: u32, ppem: u16, mode: HintMode) -> Result<BezPath, String> {
        let id = self.checked_id(glyph_id)?;
        // The face view borrows only immutable byte data. The hinter map is
        // a separate field, so a cached engine can be reused across glyphs.
        let font = FontRef::from_index(self.data.bytes(), self.data.index())
            .map_err(|error| format!("font tables became unavailable: {error}"))?;
        let glyph = font
            .outline_glyphs()
            .get(id)
            .ok_or("font glyph has no outline")?;
        // A zero cache suffix is the unhinted key shared by 1×, 2× and
        // transformed text; no hint engine is created for that common case.
        if ppem == 0 || mode == HintMode::None {
            let mut pen = EmPen {
                path: BezPath::new(),
                scale: 1.0 / f64::from(self.upem),
            };
            glyph
                .draw(
                    DrawSettings::unhinted(Size::unscaled(), &self.location),
                    &mut pen,
                )
                .map_err(|error| format!("font outline failed: {error}"))?;
            return Ok(pen.path);
        }
        Self::hinted_outline(&mut self.hinters, &self.location, &font, glyph, ppem, mode)
    }

    /// Rejects shaped IDs outside the font's validated glyph directory.
    /// A malformed GSUB table must not turn an arbitrary ID into a panic.
    fn checked_id(&self, raw: u32) -> Result<GlyphId, String> {
        if raw >= u32::from(self.glyph_count) {
            return Err("glyph ID exceeds font glyph count".into());
        }
        Ok(GlyphId::new(raw))
    }

    /// Reuses one actual skrifa hinting engine per face, mode and ppem.
    ///
    /// The interpreter and auto-hinter are intentionally distinct: the
    /// user chose light auto-hinting, while native instructions remain an
    /// explicit option for later settings. Neither can silently substitute
    /// for the other, which would make cached outlines nondeterministic.
    fn hinted_outline(
        hinters: &mut FxHashMap<(u16, HintMode), HintingInstance>,
        location: &Location,
        font: &FontRef<'_>,
        glyph: skrifa::outline::OutlineGlyph<'_>,
        ppem: u16,
        mode: HintMode,
    ) -> Result<BezPath, String> {
        let options = match mode {
            // Light smooth mode fits stems without full monochrome snapping.
            HintMode::Light => HintingOptions {
                engine: Engine::Auto(None),
                target: Target::from(SmoothMode::Light),
            },
            // Native smooth mode executes the font's own hint program.
            HintMode::Native => HintingOptions {
                engine: Engine::Interpreter,
                target: Target::from(SmoothMode::Normal),
            },
            HintMode::None => return Err("unhinted glyph requested from hinter".into()),
        };
        let key = (ppem, mode);
        // Constructing a hinting instance per glyph would pay setup cost
        // repeatedly in the first frame; reuse it for all glyphs at ppem.
        if let std::collections::hash_map::Entry::Vacant(slot) = hinters.entry(key) {
            let instance = HintingInstance::new(
                &font.outline_glyphs(),
                Size::new(f32::from(ppem)),
                location,
                options,
            )
            .map_err(|error| format!("font hinting setup failed: {error}"))?;
            slot.insert(instance);
        }
        let instance = hinters.get(&key).ok_or("hinting cache lost its entry")?;
        // Hinted callbacks are in device pixels. Divide by ppem so the
        // unchanged Slug atlas still stores coordinates in em space.
        let mut pen = EmPen {
            path: BezPath::new(),
            scale: 1.0 / f64::from(ppem),
        };
        glyph
            .draw(DrawSettings::hinted(instance, false), &mut pen)
            .map_err(|error| format!("hinted outline failed: {error}"))?;
        Ok(pen.path)
    }
}

/// Finds the three substitution features relevant to the ASCII fast path.
fn gsub_has_ascii_ligatures(font: &FontRef<'_>) -> Result<bool, String> {
    let Some(gsub) = font.table_data(skrifa::Tag::new(b"GSUB")) else {
        return Ok(false);
    };
    let bytes = gsub.as_bytes();
    // OpenType GSUB's v1 header stores FeatureListOffset at bytes 6–7.
    // Scanning only six-byte FeatureRecords avoids a new direct read-fonts
    // dependency while still rejecting truncated external font tables.
    let offset = bytes.get(6..8).ok_or("GSUB header is truncated")?;
    let offset = usize::from(u16::from_be_bytes([offset[0], offset[1]]));
    if offset == 0 {
        return Ok(false);
    }
    let list = bytes
        .get(offset..)
        .ok_or("GSUB feature list is outside table")?;
    let count = list.get(0..2).ok_or("GSUB feature count is truncated")?;
    let count = usize::from(u16::from_be_bytes([count[0], count[1]]));
    for index in 0..count {
        // A FeatureRecord has a four-byte tag followed by a two-byte
        // offset. Only the tag affects this conservative fast-path gate.
        let start = 2 + index * 6;
        let record = list
            .get(start..start + 6)
            .ok_or("GSUB feature record is truncated")?;
        if matches!(&record[..4], b"liga" | b"calt" | b"dlig") {
            return Ok(true);
        }
    }
    Ok(false)
}
