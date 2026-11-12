//! One-time cmap inversion replaces per-scalar exhaustive family scans.
//!
//! The native worker visits each installed face once, releases its mapping,
//! then publishes compact scalar offsets and ordered candidate family IDs.
//! Absent scalars have an empty slice, including after 46,607 distinct queries.
//! No negative cache needs eviction and no font bytes remain in the index.
//!
//! The offset table includes a sentinel at U+110000 so the final scalar has
//! the same half-open candidate slice as every earlier scalar. Face IDs,
//! rather than family IDs, retain differences between a family's styles.
//! Construction keeps temporary (scalar, face) pairs only until stable inversion
//! and deduplication finish. Each font mapping is released between faces, so a
//! large TTC never becomes a retained heap copy. The table and metadata costs
//! are logged separately from transient pairs and peak virtual mappings.
//! Publication transfers one immutable Arc to all native source policies;
//! size/DPR changes reuse it while supplying new device-geometry requests.
//! Color-table eligibility is collected during this same face visit, before unmapping.
use super::NativeFonts;
use fontique::FamilyId;
use std::sync::Arc;
/// Linear stable inversion avoids comparison sorting of millions of cmap pairs.
mod relation;
/// Immutable scalar-to-candidate table, independent of size and source policy.
pub(super) struct Index {
    /// Unicode scalar values index consecutive candidate slices in constant time.
    offsets: Vec<u32>,
    /// Candidate face IDs preserve style-specific character-map differences.
    families: Vec<u32>,
    /// Each entry is an actual face, so style-specific cmap differences remain visible.
    pub(super) faces: Vec<Face>,
}
/// Font metadata points to files; it never retains mapped font bytes.
pub(super) struct Face {
    /// Emoji policy can reject monochrome-only candidates without shaping every face.
    pub(super) color: bool,
    /// Native family preference remains independent of scalar lookup.
    pub(super) family: FamilyId,
    /// Face position inside the native family names diagnostic/style identity.
    pub(super) slot: usize,
    /// Loading happens only in the background preparation owner.
    pub(super) font: fontique::FontInfo,
}
impl Index {
    /// Returns no candidates for invalid, surrogate or genuinely uncovered scalars.
    pub(super) fn candidates(&self, scalar: u32) -> &[u32] {
        // The sentinel makes the last legal scalar use the same checked slicing rule.
        let Some(pair) = self.offsets.get(scalar as usize..scalar as usize + 2) else {
            return &[];
        };
        &self.families[pair[0] as usize..pair[1] as usize]
    }
    /// Rejects an unresolved cluster if any visible scalar has no installed candidate.
    pub(super) fn missing(&self, cluster: &str, emoji: bool) -> bool {
        cluster
            .chars()
            .filter(|c| !puzzle_term::text::font_set::is_default_ignorable(*c))
            .any(|c| {
                !self
                    .candidates(c as u32)
                    .iter()
                    .any(|id| !emoji || self.faces[*id as usize].color)
            })
    }
}
impl NativeFonts {
    /// Runs only in the native discovery worker or an explicitly synchronous cold probe.
    pub fn ensure_index(&mut self) {
        if self.index.is_some() {
            return;
        }
        // Enumeration and cmap parsing may block on the OS; this method is deliberately
        // confined to the background service and explicitly synchronous evidence tools.
        let started = std::time::Instant::now();
        let _index_time = super::timing::Timing::start(&self.timing, super::timing::Stage::Index);
        if !self.initialized {
            if self.system {
                self.collection.load_system_fonts();
            }
            self.initialized = true;
        }
        // Copy names before mutably resolving families; no borrowed collection string
        // survives fontique metadata loading. These temporary names leave after indexing.
        let names: Vec<String> = self.collection.family_names().map(str::to_owned).collect();
        // The relation is collected as integer pairs, avoiding one allocation per scalar.
        // Faces retain only metadata needed to load a selected candidate later.
        let mut pairs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut faces = Vec::new();
        for name in names {
            // macOS LastResort falsely advertises all of Unicode as covered by boxes.
            if name == ".LastResort" {
                continue;
            }
            let Some(family) = self.collection.family_by_name(&name) else {
                continue;
            };
            for (slot, font) in family.fonts().iter().enumerate() {
                // Family aliases can expose the same file/index more than once. Keeping
                // one face ID prevents duplicate candidate work for those aliases.
                if !seen.insert((family.id(), font.source().id(), font.index())) {
                    continue;
                }
                if let Some(blob) = font.load(Some(&mut self.files)) {
                    self.peak_mapped_bytes = self.peak_mapped_bytes.max(blob.data().len());
                    if let Ok(scalars) =
                        puzzle_term::text::cluster_font::coverage_scalars(blob.data(), font.index())
                    {
                        // Glyph zero is absent from coverage_scalars, and malformed cmaps
                        // are rejected before IDs can enter the published relation.
                        let id = faces.len() as u32;
                        pairs.extend(scalars.into_iter().map(|scalar| (scalar, id)));
                        faces.push(Face {
                            color: puzzle_term::text::cluster_font::has_color_tables(
                                blob.data(),
                                font.index(),
                            ),
                            family: family.id(),
                            slot,
                            font: font.clone(),
                        });
                    }
                }
                // One mapping at a time: collections never become retained heap copies.
                self.files.prune(0, true);
            }
        }
        self.publish_index(pairs, faces, started);
    }
    /// Packs the inverted relation once; later hits do not enumerate or open font files.
    fn publish_index(
        &mut self,
        pairs: Vec<(u32, u32)>,
        faces: Vec<Face>,
        started: std::time::Instant,
    ) {
        let transient_bytes = pairs.capacity() * std::mem::size_of::<(u32, u32)>();
        // Stable counting inversion preserves ordered candidates and exact duplicates.
        // Empty buckets remain immediate negative answers, with no evictable facts.
        let (offsets, families) = relation::invert(pairs);
        let bytes = offsets.capacity() * 4 + families.capacity() * std::mem::size_of::<u32>();
        tracing::info!(
            event = "font.coverage_index_ready",
            faces = faces.len(),
            candidates = families.len(),
            table_bytes = bytes,
            face_metadata_bytes = faces.capacity() * std::mem::size_of::<Face>(),
            transient_pairs_bytes = transient_bytes,
            transient_cursor_bytes = offsets.len() * std::mem::size_of::<u32>(),
            peak_mapped_bytes = self.peak_mapped_bytes,
            elapsed_ms = started.elapsed().as_secs_f64() * 1000.0
        );
        self.index = Some(Arc::new(Index {
            offsets,
            families,
            faces,
        }));
    }
}
