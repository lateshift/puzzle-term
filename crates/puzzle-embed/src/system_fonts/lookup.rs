//! Synchronous resolution opens only faces selected by the immutable Unicode index.
//!
//! Configured family order precedes generic/script preferences, followed by
//! other faces with matching cmap entries. No request enumerates system families.
//! Whole-cluster intersections include marks and retain presentation selectors
//! for shaping; candidate validation rejects partial ZWJ/flag ink.
//! The index names face metadata, never mapped bytes. Each opened font is parsed
//! by our skrifa/Harfrust. Successful faces stay loaded under a bounded LRU policy;
//! completed paths, color graphs and size-specific bitmaps own all their data.
//! Live frame construction calls this module on the render thread. Cold probes
//! use the same complete path; the UI never borrows its font owner.
//! Real weight/slant matching precedes synthesis, whose flags cross the seam
//! as booleans. No foreign font-engine type enters the core crates (R9).
//! The measured mapping peak is virtual storage; heap and RSS have separate scopes.
use super::{
    NativeFonts,
    timing::{Stage, Timing},
};
use fontique::{Attributes, FontStyle, FontWeight};
/// Bounded candidate validation borrows only the separate warning identities.
mod candidate;
/// Cmap intersections and family/style ordering perform no font-file work.
mod ranking;
use puzzle_term::text::{
    cluster_source::{ClusterGlyph, ClusterRequest},
    glyph::GlyphPlacement,
};

/// Cold resolution and bounded file lifetimes belong to the native source.
impl NativeFonts {
    /// Resolves the index candidates in family/style order without a per-scalar family scan.
    pub fn resolve(&mut self, r: ClusterRequest<'_>, p: &GlyphPlacement) -> Option<ClusterGlyph> {
        // Cold probes construct their index here. Live rendering has already taken
        // the startup discovery owner, waiting once if its relation was not ready.
        self.ensure_index();
        let lookup_time = Timing::start(&self.timing, Stage::Lookup);
        // Cloning the immutable owner leaves candidates stable while mappings are opened
        // and released by this render owner. The index contains metadata rather than bytes.
        let index = std::sync::Arc::clone(self.index.as_ref()?);
        let eligible = self.ranked_candidates(r, &index)?;
        // Ranking selects candidates before any mapping or outline work begins.
        drop(lookup_time);
        self.prepare_candidates(r, p, &index, &eligible)
    }
    /// Opens only index-selected faces, borrowing one mapping at a time during the build.
    /// Matching attributes affect face choice, not the lifetime of the shared file bytes.
    /// The immutable index validates these IDs when it publishes the candidate table.
    /// A cold candidate is fully validated before drawing or trying the next candidate.
    /// Style-specific extraction costs exclude file-open and unmapping time.
    /// Return the reusable charmap and hinter to their retained face owner.
    /// The first complete cluster wins; subsequent candidates would change fallback policy.
    /// Diagnostics retain the largest simultaneous virtual mapping, independent of RSS.
    /// Only the selected family name survives candidate work for review/probe reporting.
    /// LastResort reports broad coverage with boxes, so it cannot win a query.
    /// Complete shaping and ink validate selectors; cmap hits alone cannot promise
    /// an intact emoji or a valid bitmap strike.
    fn prepare_candidates(
        &mut self,
        r: ClusterRequest<'_>,
        p: &GlyphPlacement,
        index: &super::index::Index,
        ids: &[u32],
    ) -> Option<ClusterGlyph> {
        let attrs = attributes(r.style);
        let last_resort = self.collection.family_id(".LastResort");
        let mut scan = candidate::Scan::new(
            self.emoji_only,
            p.baseline_px,
            &mut self.warned,
            last_resort,
            self.timing.clone(),
        );
        for id in ids {
            // A cmap-positive but undrawable font must not be evaluated every frame.
            if self.negative.contains(r, p, *id, self.emoji_only) {
                continue;
            }
            let face = &index.faces[*id as usize];
            let Some(font) =
                self.resident
                    .loaded_face(*id, face, attrs, &mut self.files, &self.timing)
            else {
                // An unavailable candidate is also a completed negative lookup.
                // Retrying it for every cell would repeatedly reopen the same file.
                self.negative.insert(r, p, *id, self.emoji_only);
                continue;
            };
            scan.parsed = self.resident.take_parsed(*id, r.style);
            let stop = scan.visit(&font, r, p) == fontique::QueryStatus::Stop;
            if !stop {
                self.negative.insert(r, p, *id, self.emoji_only);
            }
            // Retain only complete successful ink. A capacity-driven release happens here
            // on this font owner; no borrowed table escapes or atlas address moves here.
            let unmap_time = Timing::start(&self.timing, Stage::Unmapping);
            if scan.result.is_some() {
                self.resident.retain(*id, face.font.source().id(), &font);
            }
            if let Some(parsed) = scan.parsed.take() {
                self.resident.put_parsed(*id, r.style, parsed);
            }
            drop(font);
            // SourceCache is just transient ownership. Resident faces keep the strong blob
            // reference, so clearing this cache no longer unmaps every successful request.
            self.files.prune(0, true);
            drop(unmap_time);
            if stop {
                break;
            }
        }
        self.peak_mapped_bytes = self.peak_mapped_bytes.max(scan.bytes);
        self.resident.report(&self.timing);
        let family = scan.chosen.and_then(|id| self.collection.family_name(id));
        self.last_family = family.map(str::to_owned);
        scan.result
    }
}

/// Requests actual native bold/italic faces before falling back to family defaults.
fn attributes(style: puzzle_term::text::font_set::FontStyle) -> Attributes {
    use puzzle_term::text::font_set::FontStyle as S;
    Attributes {
        weight: if matches!(style, S::Bold | S::BoldItalic) {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        },
        style: if matches!(style, S::Italic | S::BoldItalic) {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
        ..Attributes::default()
    }
}
