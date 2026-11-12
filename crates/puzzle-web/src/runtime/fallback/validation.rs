//! Device-sized validation transfers its final drawn bitmap without repeating work.
//!
//! Single-scalar jobs compare the very ink they will publish. Multi-scalar jobs
//! additionally validate unseen visible scalars once; selectors and ZWJ never
//! create standalone probes (§8.3–§8.4, reviewer decision 2026-10-06).
//!
//! CSS family order belongs to the source, so scalar coverage has no size key.
//! Presentation still distinguishes text-first from emoji-first system families.
//! Coverage storage does not contain pixels from covered glyphs.
//! Those pixels transfer directly into the completion lane and raster atlas.
//! Only known unsupported-marker samples survive in this module.
//! Their key includes the device geometry used by fitting, not just font size.
//! A resize can change fitted ink even when the CSS font size stays constant.
//! The marker LRU is independent of permanent two-bit scalar coverage facts.
//! Evicting a marker therefore causes at most one later warmup draw.
//! Multi-scalar requests reuse earlier facts across different compositions.
//! This is essential for emoji modifiers, flags and repeated ZWJ sequences.
//! Selectors influence the whole-cluster draw without claiming standalone ink.
//! No fixed-size probe can validate one size and discard its covered bitmap.
//! Phase timings distinguish the final draw from extra validation work.
//! API-call counters separately expose marker and component draws/readbacks.
use super::*;
use puzzle_term::text::glyph::RasterGlyph;

/// Missing-marker samples depend on fitting geometry as well as family/style/size.
type Key = (FontStyle, Presentation, u32, u32, u32, u32, u8);
/// Reusable unsupported ink is bounded by the existing eight sized-result slots and one mask page.
#[derive(Default)]
pub(super) struct Markers {
    /// Oldest used entry is first; no prepared covered glyph is retained here.
    entries: Vec<(Key, RasterGlyph)>,
}
/// The source retains policy facts while every prepared covered bitmap has one owner.
impl BrowserFallback {
    /// One final draw supplies both coverage validation and owned publication pixels.
    pub(super) fn prepare_covered(
        &mut self,
        r: ClusterRequest<'_>,
        p: &GlyphPlacement,
        validated: bool,
    ) -> Result<Option<ClusterGlyph>, String> {
        // Even an early known-negative return must not inherit the preceding cost.
        self.preparation_costs = [std::time::Duration::ZERO; 2];
        // Known negative facts avoid preparation; accepted ink is never drawn twice.
        if r.cluster
            .chars()
            .any(|c| !puzzle_term::text::font_set::is_default_ignorable(c) && self.fact(r, c) == 1)
        {
            return Ok(None);
        }
        // Include measurement, fitting and cropping in the final preparation phase.
        let at = web_time::Instant::now();
        let glyph = self.draw(r, p);
        self.preparation_costs[1] = at.elapsed();
        // Validation inspects this same owned bitmap, never a disposable 24px draw.
        let at = web_time::Instant::now();
        let result = glyph.and_then(|glyph| self.validate(r, p, validated, glyph));
        self.preparation_costs[0] = at.elapsed();
        // Cumulative discovery excludes the final pixels, which are already prepared.
        self.index_us += self.preparation_costs[0].as_micros() as usize;
        result
    }
    /// Whole-cluster ink alone cannot prove that a ZWJ sequence contains no missing box.
    fn validate(
        &mut self,
        r: ClusterRequest<'_>,
        p: &GlyphPlacement,
        validated: bool,
        glyph: ClusterGlyph,
    ) -> Result<Option<ClusterGlyph>, String> {
        // Borrow owned pixels for validation without cloning their backing vector.
        let ClusterGlyph::Bitmap(ref ink) = glyph else {
            return Ok(None);
        };
        // A sized retry may carry an authoritative earlier all-scalar answer.
        if !validated {
            for scalar in r
                .cluster
                .chars()
                .filter(|c| !puzzle_term::text::font_set::is_default_ignorable(*c))
            {
                // A scalar appearing in several clusters is probed only on first use.
                if self.fact(r, scalar) == 2 {
                    continue;
                }
                // A single scalar validates the final bitmap itself; only unknown
                // components of a composed cluster need an extra device-sized draw.
                let covered = if r.cluster.chars().count() == 1 {
                    self.real_ink(r, p, ink)?
                } else {
                    // Unicode scalar UTF-8 fits on the stack, avoiding an extra String.
                    let mut bytes = [0; 4];
                    let one = ClusterRequest {
                        cluster: scalar.encode_utf8(&mut bytes),
                        ..r
                    };
                    // Attribute this additional draw to validation, not final ink.
                    self.operations.validation(false);
                    match self.draw(one, p)? {
                        ClusterGlyph::Bitmap(ref component) => self.real_ink(one, p, component)?,
                        _ => false,
                    }
                };
                let slot = slot(r, scalar);
                // Each policy/scalar owns exactly two permanent bits (§8.3).
                let shift = (slot % 4) * 2;
                // Unknown slots contain zero; supported=2 and unsupported=1 persist.
                self.scalars[slot / 4] |= (if covered { 2 } else { 1 }) << shift;
                self.indexed_slots += 1;
                // Reject the complete cluster if any visible component is missing.
                if !covered {
                    return Ok(None);
                }
            }
        }
        // The drawn cluster is transferred, not retained in a validation side cache.
        Ok(Some(glyph))
    }
    /// Coverage is a permanent policy fact independent of cluster retry/retirement.
    fn fact(&self, r: ClusterRequest<'_>, scalar: char) -> u8 {
        // Four two-bit answers share a byte; policy offsets cannot overlap.
        let slot = slot(r, scalar);
        (self.scalars[slot / 4] >> ((slot % 4) * 2)) & 3
    }
    /// Compare cropped device-sized ink to the matching once-drawn unsupported marker.
    fn real_ink(
        &mut self,
        r: ClusterRequest<'_>,
        p: &GlyphPlacement,
        ink: &RasterGlyph,
    ) -> Result<bool, String> {
        // Preserve f32 identity instead of rounding sizes or baseline offsets.
        let key = (
            r.style,
            r.presentation,
            p.size_px.to_bits(),
            p.ink_limit_px,
            p.cell_height_px,
            p.baseline_px.to_bits(),
            p.cell_span,
        );
        // The small bounded list avoids retaining covered glyphs as a side cache.
        let index = if let Some(i) = self.markers.entries.iter().position(|(k, _)| *k == key) {
            i
        } else {
            if !self.warm_marker(r, p, key)? {
                return Ok(false);
            }
            self.markers.entries.len() - 1
        };
        // Reinsert the compared sample last to update its independent LRU recency.
        let entry = self.markers.entries.remove(index);
        let missing = &entry.1;
        // Compare geometry and pixels; two equally sized boxes remain unsupported.
        let real = ink.kind != missing.kind
            || ink.width != missing.width
            || ink.height != missing.height
            || ink.offset_px != missing.offset_px
            || ink.pixels != missing.pixels;
        self.markers.entries.push(entry);
        // Transparent samples cannot be accepted merely because their bounds differ.
        Ok(real && ink.pixels.iter().any(|v| *v != 0))
    }
    /// Draw one unsupported marker at the exact geometry and retain it within bounds.
    fn warm_marker(
        &mut self,
        r: ClusterRequest<'_>,
        p: &GlyphPlacement,
        key: Key,
    ) -> Result<bool, String> {
        // Warmup is measured separately from each newly validated scalar.
        self.operations.validation(true);
        let marker = self.draw(
            ClusterRequest {
                cluster: "\u{10ffff}",
                ..r
            },
            p,
        )?;
        // Canvas sources return rasters; other representations cannot be compared here.
        let ClusterGlyph::Bitmap(marker) = marker else {
            return Ok(false);
        };
        // Marker bytes fit within one existing 2048² mask page. Evict oldest
        // samples instead of growing memory with font zoom or style changes.
        while self.markers.entries.len() >= 8
            || self
                .markers
                .entries
                .iter()
                .map(|(_, v)| v.pixels.len())
                .sum::<usize>()
                + marker.pixels.len()
                > 2048 * 2048
        {
            // Reject an oversized marker rather than silently raising the byte bound.
            if self.markers.entries.is_empty() {
                return Err("unsupported marker exceeds mask page".into());
            }
            // Oldest-use retirement affects neither prepared ink nor coverage facts.
            self.markers.entries.remove(0);
        }
        // The newly drawn marker is now the most recently used sample.
        self.markers.entries.push((key, marker));
        Ok(true)
    }
}
/// Presentation remains part of policy because it selects a different CSS family order.
fn slot(r: ClusterRequest<'_>, scalar: char) -> usize {
    // Each of four styles has independent text and emoji coverage planes.
    (r.style as usize * 2 + usize::from(r.presentation == Presentation::Emoji)) * 0x110000
        + scalar as usize
}
/// Real Canvas fixtures protect the no-discard and all-visible-scalars contracts.
#[cfg(test)]
mod tests;
