//! Shaped glyph IDs and cluster offsets for terminal runs (design §8.4).
//!
//! Harfrust applies font substitutions such as JetBrains Mono's coding
//! ligatures. Its advances are deliberately discarded: a terminal cell
//! grid, not proportional font metrics, determines cursor movement.
//! Cluster byte offsets survive shaping so the layout layer can place a
//! ligature at its first cell and retain the full occupied cell span.
//!
//! Printable ASCII bypasses Harfrust when ligatures are disabled or the
//! face has no GSUB table. The output is still expressed in the same
//! `ShapedGlyph` type. A bounded cache stores full-text results, and the
//! Unicode buffer is recycled after every shaping call.
use crate::face::FontFace;
use harfrust::{BufferFlags, Direction, Feature, ShapeOptions, UnicodeBuffer};
use rustc_hash::{FxHashMap, FxHasher};
use std::hash::{Hash, Hasher};

/// One glyph selected by shaping before grid placement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph {
    /// OpenType glyph ID in the run's selected face.
    pub glyph_id: u32,
    /// UTF-8 byte offset to the start of the source cluster.
    pub cluster: usize,
    /// Natural shaping advance for free text; grid placement deliberately ignores it.
    /// None denotes the ASCII cmap path, whose face supplies a design advance.
    pub advance_em: Option<f32>,
    /// Mark or attachment shift measured in ems, not cells.
    pub x_offset_em: f32,
    /// Vertical mark shift in ems, positive toward the font's ascent.
    pub y_offset_em: f32,
}

/// One user-requested OpenType feature value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FeatureSetting {
    /// Four-byte OpenType tag such as `ss01` or `zero`.
    pub tag: [u8; 4],
    /// Zero disables the feature; nonzero values select alternatives.
    pub value: u32,
}

/// Settings that can change substitutions or cluster positions.
#[derive(Clone, Copy)]
pub struct ShapingOptions<'a> {
    /// Enables the font's normal `liga` and contextual `calt` features.
    pub ligatures: bool,
    /// Additional caller-specified OpenType feature values.
    pub features: &'a [FeatureSetting],
}

/// Cached run result with an exact key to defend against hash collisions.
struct CacheEntry {
    /// Face index in the active font set.
    face_id: u32,
    /// Whether default coding ligatures were enabled.
    ligatures: bool,
    /// Feature settings copied only on a cache miss.
    features: Vec<FeatureSetting>,
    /// UTF-8 text that produced the glyphs.
    text: String,
    /// Cached IDs and offsets independent of the current grid position.
    glyphs: Vec<ShapedGlyph>,
    /// Monotone access sequence for 4096-entry LRU eviction.
    last_used: u64,
}

/// Reuses Harfrust buffers and bounded shaped-run results.
pub struct TextShaper {
    /// Buffer returned by `GlyphBuffer::clear` after shaping.
    buffer: UnicodeBuffer,
    /// Exact entries indexed by a cheap noncryptographic lookup hash.
    cache: FxHashMap<u64, CacheEntry>,
    /// Number of uses, which orders least-recently-used eviction.
    access: u64,
    /// Optional cumulative lookups copied only by diagnostic hosts.
    pub(crate) metrics: Option<crate::cache_metrics::CacheMetrics>,
}

/// Creates an empty shaper without loading a font or allocating a run.
impl Default for TextShaper {
    /// Keeps expensive Harfrust data in each face until first use.
    fn default() -> Self {
        Self {
            buffer: UnicodeBuffer::new(),
            cache: FxHashMap::default(),
            access: 0,
            metrics: None,
        }
    }
}

/// Implements the direct path, Harfrust path and 4096-entry run cache.
impl TextShaper {
    /// Shapes one equal-face run and returns its cached result by borrow.
    pub fn shape<'a>(
        &'a mut self,
        face_id: u32,
        face: &FontFace,
        text: &str,
        options: ShapingOptions<'_>,
    ) -> Result<&'a [ShapedGlyph], String> {
        // The lookup hash is only an index. Font text may be untrusted,
        // and two different runs can share a hash, so a hit needs the
        // complete face, feature and text comparison below.
        let hash = run_hash(face_id, text, options);
        // The access clock saturates rather than wrapping; a wrap would
        // make a recently used entry look older than every stale one.
        self.access = self.access.saturating_add(1);
        let hit = self.cache.get(&hash).is_some_and(|entry| {
            entry.face_id == face_id
                && entry.ligatures == options.ligatures
                && entry.features == options.features
                && entry.text == text
        });
        crate::cache_metrics::CacheMetrics::lookup(&mut self.metrics, hit, text.len());
        if hit {
            // A cache result contains offsets but no absolute position.
            // It can therefore serve the same run at a different cell x.
            let entry = self
                .cache
                .get_mut(&hash)
                .ok_or("shaped-run cache lost its hit")?;
            entry.last_used = self.access;
            return Ok(&entry.glyphs);
        }
        // Collisions replace an entry only after full-key comparison. The
        // wrong glyph sequence is never returned for unrelated input.
        let glyphs = self.shape_uncached(face, text, options)?;
        // Eviction occurs only after a successful shape. A broken font
        // must not displace a previously valid result from the cache.
        if self.cache.len() >= 4096 && !self.cache.contains_key(&hash) {
            self.evict_oldest();
        }
        self.cache.insert(
            hash,
            CacheEntry {
                // Own the exact key at insertion, once per distinct run.
                // Cache hits avoid these allocations entirely.
                face_id,
                ligatures: options.ligatures,
                features: options.features.to_vec(),
                text: text.to_owned(),
                glyphs,
                last_used: self.access,
            },
        );
        Ok(&self
            .cache
            .get(&hash)
            .ok_or("shaped run was not cached")?
            .glyphs)
    }

    /// Counts a direct ASCII run as a distinct shaping path for budgets.
    pub fn ascii_fast_path(
        face: &FontFace,
        text: &str,
        options: ShapingOptions<'_>,
    ) -> Result<bool, String> {
        // ASCII cmap lookup is valid only for printable single-byte
        // scalars. Tabs, controls and Unicode need cluster-aware shaping.
        if !text.bytes().all(|byte| (0x20..=0x7e).contains(&byte)) || !options.features.is_empty() {
            return Ok(false);
        }
        // Some fonts have GSUB for unrelated scripts but no ASCII coding
        // features. The face scans liga/calt/dlig once on load, so this
        // run can use direct cmap lookup without a table parse here.
        Ok(!options.ligatures || !face.has_ascii_ligatures())
    }

    /// Produces a run without touching the run cache.
    fn shape_uncached(
        &mut self,
        face: &FontFace,
        text: &str,
        options: ShapingOptions<'_>,
    ) -> Result<Vec<ShapedGlyph>, String> {
        if Self::ascii_fast_path(face, text, options)? {
            // Each ASCII byte is one UTF-8 scalar and one terminal cell;
            // its byte index is the cluster-to-cell map used by layout.
            return text
                .bytes()
                .enumerate()
                .map(|(cluster, byte)| {
                    Ok(ShapedGlyph {
                        glyph_id: face.ascii_glyph(byte).ok_or("ASCII glyph is missing")?,
                        cluster,
                        advance_em: None,
                        x_offset_em: 0.0,
                        y_offset_em: 0.0,
                    })
                })
                .collect();
        }
        self.shape_harfrust(face, text, options)
    }

    /// Applies substitutions and mark offsets, then recycles the buffer.
    fn shape_harfrust(
        &mut self,
        face: &FontFace,
        text: &str,
        options: ShapingOptions<'_>,
    ) -> Result<Vec<ShapedGlyph>, String> {
        let font = face.font()?;
        let shaper = face
            .shaper_data()
            .shaper(&font)
            .instance(Some(face.shaper_instance()))
            .build();
        // The UnicodeBuffer is owned by this shaper between calls. Taking
        // it permits Harfrust to mutate and then return the allocation.
        let mut buffer = std::mem::take(&mut self.buffer);
        buffer.push_str(text);
        // Harfrust's default replaces default-ignorables with a zero-advance
        // space glyph. Grid placement ignores that advance and would paint
        // the space or tofu at the emoji origin. Let Harfrust shape with
        // selectors intact, then remove their output glyphs (§8.4).
        buffer.set_flags(BufferFlags::REMOVE_DEFAULT_IGNORABLES);
        // Script inference selects script-specific OpenType substitutions.
        // Direction remains LTR because terminal cells progress leftward
        // to rightward even when a run contains complex glyph clusters.
        buffer.guess_segment_properties();
        buffer.set_direction(Direction::LeftToRight);
        let features = shaping_features(options);
        // Harfrust's default feature plan already enables ordinary liga
        // and calt. Explicit zeroes above are only needed for the off mode.
        let shaped = shaper.shape(buffer, ShapeOptions::new().features(&features));
        let upem = shaper.units_per_em() as f32;
        // A malformed font with zero UPEM cannot yield usable em offsets.
        // Clear first so the next valid run can reuse the same buffer.
        if upem <= 0.0 {
            self.buffer = shaped.clear();
            return Err("shaper has zero units per em".into());
        }
        let result = shaped
            .glyph_infos()
            .iter()
            .zip(shaped.glyph_positions())
            .map(|(info, pos)| ShapedGlyph {
                // Harfrust gives byte clusters. Keeping them unchanged
                // allows the grid layer to associate an N-cell ligature
                // with the first source cell instead of its font advance.
                glyph_id: info.glyph_id,
                cluster: info.cluster as usize,
                advance_em: Some(pos.x_advance as f32 / upem),
                x_offset_em: pos.x_offset as f32 / upem,
                y_offset_em: pos.y_offset as f32 / upem,
            })
            .collect();
        // The shaped output owns the original UnicodeBuffer allocation;
        // recycling it avoids a fresh text buffer on the next dirty row.
        self.buffer = shaped.clear();
        Ok(result)
    }

    /// Removes the least recently read result before a cache insertion.
    fn evict_oldest(&mut self) {
        // Scanning 4096 entries happens only on a cache miss at capacity;
        // cache hits and the steady ASCII path never pay this cost.
        if let Some(hash) = self
            .cache
            .iter()
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(hash, _)| *hash)
        {
            self.cache.remove(&hash);
            if let Some(metrics) = &mut self.metrics {
                metrics.evictions += 1;
            }
        }
    }
}

/// Builds OpenType overrides while leaving ordinary ligatures at font defaults.
fn shaping_features(options: ShapingOptions<'_>) -> Vec<Feature> {
    let mut features: Vec<Feature> = options
        .features
        .iter()
        .map(|setting| Feature::new(harfrust::Tag::new(&setting.tag), setting.value, ..))
        .collect();
    // Disabling all coding ligature families matters: JetBrains Mono
    // can form arrows through contextual alternatives as well as liga.
    if !options.ligatures {
        for tag in [b"liga", b"calt", b"dlig"] {
            features.push(Feature::new(harfrust::Tag::new(tag), 0, ..));
        }
    }
    features
}

/// Hashes lookup inputs without allocating a temporary owned key.
fn run_hash(face_id: u32, text: &str, options: ShapingOptions<'_>) -> u64 {
    let mut hash = FxHasher::default();
    face_id.hash(&mut hash);
    text.hash(&mut hash);
    options.ligatures.hash(&mut hash);
    options.features.hash(&mut hash);
    hash.finish()
}

#[cfg(test)]
mod tests;

impl TextShaper {
    /// Enables lookup observation without resetting prior cumulative samples.
    pub fn measure_cache(&mut self) {
        self.metrics.get_or_insert_with(Default::default);
    }
    /// Returns cumulative shape-key facts without borrowing cached glyph vectors.
    pub fn cache_metrics(&self) -> crate::cache_metrics::CacheMetrics {
        self.metrics.unwrap_or_default()
    }
}
