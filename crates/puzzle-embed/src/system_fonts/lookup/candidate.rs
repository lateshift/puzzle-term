//! Cold candidate validation borrows loaded bytes and transfers complete owned ink.
//!
//! The candidate budget counts distinct blobs, so collection faces sharing a
//! file do not inflate mapped bytes. The callback sees an already loaded blob;
//! the bound can be crossed by one file before it stops further preparation.
//! Fontique's matches carry foreign metadata, but only bytes and scalar style
//! flags enter our parser. A malformed candidate continues to the next family.
//! Unsuccessful mappings are pruned after each candidate; successful faces
//! stay loaded under a separate bounded LRU owner. Collection faces share one blob.
//! Font parsing and all table accesses occur before the callback publishes ink.
//! A source cannot report coverage merely because its charmap maps a scalar:
//! the complete shaped cluster must yield visible geometry or decoded pixels.
//! This excludes missing-outline and broken PNG candidates from cached choices.
//! Native styles select actual faces before synthesis is considered. A fallback
//! does not inherit the primary family's missing-bold or missing-italic policy.
//! Diagnostic storage is capped independently of coverage and atlas storage.
//! Accepted ink is owned; releasing discovery files cannot invalidate GPU data.
use fontique::{FamilyId, QueryFont, QueryStatus};
use puzzle_term::text::{
    cluster_font,
    cluster_source::{ClusterGlyph, ClusterRequest},
    glyph::GlyphPlacement,
};
use std::collections::HashSet;
/// One bounded query's counters, result and coalesced font diagnostics.
pub(super) struct Scan<'a> {
    /// Parsing ownership returns to the successful resident mapping after each call.
    /// Predictable extraction excludes first-use parser setup while diagnostics retain it.
    pub(super) call_cost: std::time::Duration,
    /// A successfully parsed owner returns to resident font metadata.
    pub(super) parsed: Option<cluster_font::loaded::Face>,
    /// Emoji sources accept color paint or bitmaps, never ordinary outlines.
    emoji: bool,
    /// Optional stage owner adds no font data or lifetime dependency.
    timing: Option<std::sync::Arc<super::super::timing::Timing>>,
    /// macOS missing-glyph boxes advertise coverage but never constitute a real match.
    last_resort: Option<FamilyId>,
    /// Primary baseline, independent of the candidate's ascender.
    baseline: f32,
    /// Distinct mapped blobs retained by fontique during this query.
    mapped: Vec<u64>,
    /// Observed virtual bytes, distinct from copied heap allocations.
    pub(super) bytes: usize,
    /// Accepted family for the installed-font evidence log.
    pub(super) chosen: Option<FamilyId>,
    /// Owned ink only; no table or foreign metadata crosses the seam.
    pub(super) result: Option<ClusterGlyph>,
    /// Native font identities cap repeated diagnostic output.
    warned: &'a mut HashSet<(FamilyId, usize)>,
}
/// Query state borrows warning storage independently of the fontique collection.
impl<'a> Scan<'a> {
    /// Creates empty counters before the first candidate is mapped.
    pub(super) fn new(
        emoji: bool,
        baseline: f32,
        warned: &'a mut HashSet<(FamilyId, usize)>,
        last_resort: Option<FamilyId>,
        timing: Option<std::sync::Arc<super::super::timing::Timing>>,
    ) -> Self {
        Self {
            parsed: None,
            call_cost: std::time::Duration::ZERO,
            emoji,
            timing,
            last_resort,
            baseline,
            mapped: Vec::new(),
            bytes: 0,
            chosen: None,
            result: None,
            warned,
        }
    }
    /// Accepts only complete drawable ink and bounds cold file traversal.
    pub(super) fn visit(
        &mut self,
        font: &QueryFont,
        r: ClusterRequest<'_>,
        p: &GlyphPlacement,
    ) -> QueryStatus {
        if !self.mapped.contains(&font.blob.id()) {
            // Candidate traversal charges each file once even when its collection has faces.
            self.mapped.push(font.blob.id());
            self.bytes = self.bytes.saturating_add(font.blob.data().len());
        }
        // Count and virtual bytes constrain traversal; font blobs are never copied.
        if self.mapped.len() > 32 || self.bytes > 512 * 1024 * 1024 {
            return QueryStatus::Stop;
        }
        // LastResort maps all Unicode to boxes and would conceal genuine later coverage.
        if Some(font.family.0) == self.last_resort {
            return QueryStatus::Continue;
        }
        let Some(ink) = self.ink(font, r, p) else {
            return QueryStatus::Continue;
        };
        // Only an accepted complete drawable records a family as successful.
        // Owned geometry survives the enclosing query's mapping release.
        self.chosen = Some(font.family.0);
        self.result = Some(ink);
        QueryStatus::Stop
    }
    /// Parses and styles one complete candidate without retaining foreign views.
    fn ink(
        &mut self,
        font: &QueryFont,
        r: ClusterRequest<'_>,
        p: &GlyphPlacement,
    ) -> Option<ClusterGlyph> {
        if self.parsed.is_none() {
            // First-use plans and cmap selection are created only by the worker.
            let variations: Vec<_> = font
                .synthesis
                .variation_settings()
                .iter()
                .map(|(tag, value)| (tag.to_be_bytes(), *value))
                .collect();
            self.parsed =
                cluster_font::loaded::Face::new(font.blob.data(), font.index, r, &variations).ok();
        }
        let parsed = self.parsed.as_ref()?;
        let predictable = std::time::Instant::now();
        // Full coverage is checked with our version of skrifa. A fontique charmap
        // could be from its second read-fonts version and is deliberately ignored.
        let data = font.blob.data();
        // cmap validation borrows tables; it extracts no outline and decodes no bitmap.
        let coverage_time = super::super::timing::Timing::start(
            &self.timing,
            super::super::timing::Stage::Coverage,
        );
        if !parsed.covers(data, r.cluster) {
            return None;
        }
        drop(coverage_time);
        // The successful owned result below is the drawn ink, rather than a boolean probe.
        // Decoding, COLR traversal and hinting complete before we accept a family.
        // Failed preparation never publishes partial atlas data or ends a frame.
        // Convert foreign tags to four bytes; our aligned parser reconstructs its own tags.
        // Variable slant must affect shaping and paths together; a shear is used only
        // when native metadata explicitly requests synthesis for a missing real style.
        let extract_time = super::super::timing::Timing::start(
            &self.timing,
            super::super::timing::Stage::Extraction,
        );
        let ink = match parsed.prepare(data, r, p, &mut crate::bitmap::Decoder, self.baseline) {
            Ok(ink) => ink,
            Err(error) => {
                // The caller caches this exact failed candidate before trying the next.
                tracing::warn!(event="font.system_candidate_failed", cluster=r.cluster,
                    family=?font.family, %error, "system font candidate diagnostic");
                return None;
            }
        };
        drop(extract_time);
        self.call_cost = predictable.elapsed();
        // Styling attaches synthesis metadata while preserving the prepared path or pixels.
        self.styled(font, ink)
    }
    /// Applies the native synthesis policy after validating a complete candidate.
    fn styled(&mut self, font: &QueryFont, ink: ClusterGlyph) -> Option<ClusterGlyph> {
        // The dedicated emoji seam must not preempt text with a monochrome face.
        // General platform fallback may still use that face later in the chain.
        if self.emoji && !matches!(ink, ClusterGlyph::Bitmap(_) | ClusterGlyph::Color(_)) {
            return None;
        }
        // Unsupported compositing is the same explicit source-over approximation
        // as bundled COLR; the diagnostic is tied to a font rather than a cluster.
        if let ClusterGlyph::Color(ref color) = ink
            && color.unsupported_composite
        {
            self.warn(
                font,
                "font.unsupported_composite",
                "rendering system COLR composite as source-over",
            );
        }
        // Native matching picks real faces; only missing styles need extra instance ink.
        Some(match ink {
            ClusterGlyph::Outline(path) => ClusterGlyph::StyledOutline {
                path,
                bold: font.synthesis.embolden(),
                italic: font.synthesis.skew().is_some(),
            },
            other => other,
        })
    }
    /// Warns once per font under the bounded diagnostic identity allowance.
    fn warn(&mut self, font: &QueryFont, event: &str, error: &str) {
        if self.warned.len() < 256 && self.warned.insert(font.family) {
            tracing::warn!(event,%error,"system font candidate diagnostic");
        }
    }
}
