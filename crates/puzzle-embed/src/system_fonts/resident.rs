//! Loaded successful faces retain their mappings across unrelated glyph jobs.
//!
//! This is font ownership, not a cache of validation geometry. Ink extraction
//! borrows the same mapped bytes on every request. Collection faces share a
//! mapping; byte accounting charges each source once. Capacity retirement is LRU
//! and needs no idle timer or polling. Limits initially use the native discovery
//! allowance; release workload reports must record their observed peaks (P13).
use fontique::{QueryFont, SourceId};

/// Maximum simultaneously loaded successful faces, independent of glyph count.
const FACE_LIMIT: usize = 32;
/// Virtual mapped-byte ceiling; these bytes are not a retained heap copy or RSS.
const BYTE_LIMIT: usize = 512 * 1024 * 1024;

/// Successful face metadata and strong mapped-byte owners in recency order.
#[derive(Default)]
pub(super) struct Resident {
    /// Least recently used face is first; at most FACE_LIMIT entries are retained.
    faces: Vec<Entry>,
    /// Count and byte limits also support small-budget lifetime contract tests.
    limits: [usize; 2],
    /// Number of capacity-driven face releases, reported separately from mappings.
    pub(super) evictions: usize,
}

/// A face is distinct from its file because TTC collections contain many faces.
struct Entry {
    /// Stable index face identity, independent of request style and device size.
    id: u32,
    /// Shared file identity makes TTC byte accounting and reuse exact.
    source: SourceId,
    /// Strong byte owner preserves the mapping; table views never escape extraction.
    font: QueryFont,
    /// Owned parsing state is retained for each matched style/variation.
    parsed: std::collections::HashMap<
        puzzle_term::text::font_set::FontStyle,
        puzzle_term::text::cluster_font::loaded::Face,
    >,
}

impl Resident {
    /// Contract tests force retirement without depending on installed font sizes.
    #[cfg(test)]
    pub(super) fn bounded(faces: usize, bytes: usize) -> Self {
        Self {
            limits: [faces, bytes],
            ..Self::default()
        }
    }
    /// Uses the existing native traversal ceilings until workload measurements report peaks.
    pub(super) fn new() -> Self {
        Self {
            limits: [FACE_LIMIT, BYTE_LIMIT],
            ..Self::default()
        }
    }

    /// Reuses an exact loaded face and stamps its recency in preparation order.
    pub(super) fn get(&mut self, id: u32) -> Option<QueryFont> {
        // Exact indexed identity distinguishes collection faces sharing the same file.
        let slot = self.faces.iter().position(|entry| entry.id == id)?;
        // Moving one tiny metadata owner is bounded by the loaded-face allowance.
        let entry = self.faces.remove(slot);
        // QueryFont's blob clone shares ownership; it does not remap or copy font bytes.
        let font = entry.font.clone();
        // Recency follows successful use rather than mapping creation time.
        self.faces.push(entry);
        // Validation decides whether this transient candidate joins resident ownership.
        Some(font)
    }

    /// A different face in the same collection reads the existing byte mapping.
    pub(super) fn same_file(&self, source: SourceId) -> Option<QueryFont> {
        // The caller substitutes the requested collection index before parsing tables.
        self.faces
            .iter()
            .find(|entry| entry.source == source)
            .map(|entry| entry.font.clone())
    }

    /// Keeps only successfully validated faces; malformed candidates stay transient.
    pub(super) fn retain(&mut self, id: u32, source: SourceId, font: &QueryFont) {
        // A repeated success must not create two strong owners for one indexed face.
        if self.faces.iter().any(|entry| entry.id == id) {
            return;
        }
        // An individually oversized mapping may draw, but cannot become resident.
        if font.blob.data().len() > self.limits[1] || self.limits[0] == 0 {
            return;
        }
        self.faces.push(Entry {
            id,
            source,
            // Cloning the blob preserves the original mapping without reopening it.
            font: font.clone(),
            // Parsing is retained lazily after this face actually prepares ink.
            parsed: Default::default(),
        });
        // Capacity, rather than a glyph corpus or frame-count window, determines retirement.
        while self.faces.len() > self.limits[0] || self.bytes() > self.limits[1] {
            // Dropping the final file owner releases its mapping; sibling faces retain theirs.
            self.faces.remove(0);
            self.evictions += 1;
        }
    }

    /// Takes parsing ownership for a call, then returns it to the same resident entry.
    pub(super) fn take_parsed(
        &mut self,
        id: u32,
        style: puzzle_term::text::font_set::FontStyle,
    ) -> Option<puzzle_term::text::cluster_font::loaded::Face> {
        // The exact face and style own parsing, even when a sibling shares mapped bytes.
        self.faces
            .iter_mut()
            .find(|e| e.id == id)?
            .parsed
            .remove(&style)
    }
    /// Only a successful loaded mapping may retain the matching parsing metadata.
    pub(super) fn put_parsed(
        &mut self,
        id: u32,
        style: puzzle_term::text::font_set::FontStyle,
        parsed: puzzle_term::text::cluster_font::loaded::Face,
    ) {
        // A capacity-retired face cannot regain metadata without its byte owner.
        if let Some(entry) = self.faces.iter_mut().find(|e| e.id == id) {
            // Return the same engine after extraction, preserving cmap and shaper setup.
            entry.parsed.insert(style, parsed);
        }
    }
    /// Distinct source files are charged once even if several TTC faces are loaded.
    pub(super) fn bytes(&self) -> usize {
        // A bounded face set makes this scan cheaper than maintaining another owner map.
        self.faces
            .iter()
            .enumerate()
            .filter(|(i, entry)| {
                !self.faces[..*i]
                    .iter()
                    .any(|earlier| earlier.source == entry.source)
            })
            .map(|(_, entry)| entry.font.blob.data().len())
            .sum()
    }

    /// Reports current owners as well as lifetime peaks through the diagnostic counters.
    pub(super) fn report(&self, timing: &Option<std::sync::Arc<super::timing::Timing>>) {
        // Observational counters do not alter mapping retirement or byte ownership.
        if let Some(timing) = timing {
            timing.resident(self.faces.len(), self.bytes(), self.evictions);
        }
    }
    /// Loads a face from resident ownership or one transient OS file mapping.
    pub(super) fn loaded_face(
        &mut self,
        id: u32,
        face: &super::index::Face,
        attrs: fontique::Attributes,
        files: &mut fontique::SourceCache,
        timing: &Option<std::sync::Arc<super::timing::Timing>>,
    ) -> Option<fontique::QueryFont> {
        // A disappeared or unreadable font is a recoverable candidate failure.
        // Only the render owner opens fonts; the session thread publishes cells, not ink.
        let loaded = self.get(id).or_else(|| {
            // Other faces of a TTC borrow the same mapping, even on their first use.
            self.same_file(face.font.source().id())
        });
        // The chosen owner remains alive through every subsequent table and outline borrow.
        let blob = if let Some(loaded) = loaded {
            // Moving this strong owner keeps the same OS mapping alive through extraction.
            loaded.blob
        } else {
            // This timer counts actual mapping misses, rather than all glyph requests.
            let _map_time = super::timing::Timing::start(timing, super::timing::Stage::Mapping);
            // OS errors reject this candidate while leaving other indexed faces eligible.
            face.font.load(Some(files))?
        };
        // Fontique supplies only the matched metadata and byte owner. Our parsers handle
        // outlines, color layers and bitmap strikes across the crate boundary.
        let font = fontique::QueryFont {
            // The family slot carries original style metadata for synthesis decisions.
            family: (face.family, face.slot),
            blob,
            index: face.font.index(),
            // Synthesis is face-specific even when several faces share a TTC mapping.
            synthesis: face.font.synthesis(attrs.width, attrs.style, attrs.weight),
            charmap_index: face.font.charmap_index(),
        };
        // The caller retains this owner only after complete shaping and ink validation.
        // Validation decides whether this transient candidate joins resident ownership.
        Some(font)
    }
}
