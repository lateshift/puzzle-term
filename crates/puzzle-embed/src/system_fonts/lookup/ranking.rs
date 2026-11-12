//! Candidate ordering uses immutable cmap facts before any font file is opened.
//!
//! Configured families precede platform preferences; matching style breaks ties.
//! Whole-cluster intersections preserve selectors and reject partial coverage.
use super::{NativeFonts, attributes};
use fontique::GenericFamily;
use puzzle_term::text::{cluster_source::ClusterRequest, presentation::Presentation};

impl NativeFonts {
    /// Returns only eligible face IDs, ranked without mapping or extracting font ink.
    pub(super) fn ranked_candidates(
        &mut self,
        r: ClusterRequest<'_>,
        index: &super::super::index::Index,
    ) -> Option<Vec<u32>> {
        // Default-ignorable selectors/joiners influence shaping but have no ink of their own.
        // An entirely ignorable cluster therefore needs no fallback face.
        let first = r
            .cluster
            .chars()
            .find(|c| !puzzle_term::text::font_set::is_default_ignorable(*c))?;
        // The inverted cmap provides candidate IDs without touching font files.
        let candidates = index.candidates(first as u32);
        // Whole-cluster intersections discard families that cover only the base scalar.
        // Emoji-only requests exclude faces without any supported color table before
        // file loading; table presence is only eligibility, never proof of drawable ink.
        let mut eligible: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|id| {
                (!self.emoji_only || index.faces[*id as usize].color)
                    && r.cluster.chars().all(|c| {
                        puzzle_term::text::font_set::is_default_ignorable(c)
                            || index.candidates(c as u32).contains(id)
                    })
            })
            .collect();
        // An empty intersection is a complete negative answer from the cmap index.
        // Searching unrelated fonts cannot make them cover a missing visible scalar.
        if eligible.is_empty() {
            return None;
        }
        let preferred = self.preferred_families(r);
        // The scalar table retains faces, including ones omitted by script-family defaults.
        let attributes = attributes(r.style);
        // Face-level cmap differences survive family matching; only faces covering the whole
        // cluster are eligible. Within a family, native weight/slant precede synthesis.
        eligible.sort_by_key(|id| {
            let face = &index.faces[*id as usize];
            // An unpreferred but covered family remains eligible after all preferred ones.
            let family_rank = preferred
                .iter()
                .position(|family| *family == face.family)
                .unwrap_or(preferred.len());
            // Authored slant takes priority over synthesizing a shear of another face.
            let style_rank = usize::from(face.font.style() != attributes.style);
            // Closest authored weight precedes the stable collection slot tie-breaker.
            let weight_rank = (face.font.weight().value() - attributes.weight.value()).abs() as u32;
            (family_rank, style_rank, weight_rank, face.slot)
        });
        Some(eligible)
    }
    /// Preserves authored family order before platform script preferences.
    fn preferred_families(&mut self, r: ClusterRequest<'_>) -> Vec<fontique::FamilyId> {
        // Caller-selected families keep their configuration order, even when another
        // installed family has a cheaper outline or more extensive Unicode coverage.
        let mut preferred: Vec<_> = self
            .families
            .iter()
            .filter_map(|name| self.collection.family_id(name))
            .collect();
        // Emoji policy prefers authored color faces; ordinary text begins with UI sans.
        // Generic preferences rank only indexed candidates and never trigger file loading.
        preferred.extend(self.collection.generic_families(
            if self.emoji_only || r.presentation == Presentation::Emoji {
                GenericFamily::Emoji
            } else {
                GenericFamily::SansSerif
            },
        ));
        // Script preferences remain a ranking hint. The index still includes symbol faces
        // that a platform script list omits, preserving uncommon-character coverage.
        let script = puzzle_term::text::cluster_font::script(r.cluster);
        if self.scripts.insert(script) {
            // First-use OS policy is discovered lazily on the worker, never at startup
            // for unused scripts and never by an admitted frame-time call.
            let _preferences = super::super::timing::Timing::start(
                &self.timing,
                super::super::timing::Stage::Preferences,
            );
            let tag = fontique::Script::from_bytes(script);
            let families: Vec<_> = self.collection.fallback_families(tag).collect();
            // Store the OS result so subsequent calls use immutable resolved preferences.
            self.collection.set_fallbacks(tag, families.into_iter());
        }
        preferred.extend(
            self.collection
                .fallback_families(fontique::Script::from_bytes(script)),
        );
        preferred
    }
}
