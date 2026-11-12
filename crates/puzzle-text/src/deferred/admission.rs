//! A cold first draw consumes admitted ink directly, or queues the identical request.
//!
//! The host supplies time slack, and the platform decides whether its source is
//! immediately available. Accepted background work is never duplicated. Synchronous
//! validation publishes a fact while transferring its owned geometry to this draw;
//! it does not enter a second completion lane or retain a validation side cache.
use super::*;

impl Deferred {
    /// Gives one cold variant an immediate opportunity before bounded queue admission.
    pub(super) fn cold_ink(
        &mut self,
        r: ClusterRequest<'_>,
        p: &GlyphPlacement,
        hash: u64,
        size: Size,
    ) -> Result<ClusterGlyph, String> {
        let e = self.entries.get_mut(&hash).ok_or("cold request absent")?;
        // A pending size owns its validation. Another size waits for that answer.
        if e.ink.contains_key(&size) || e.covered != Some(true) && !e.ink.is_empty() {
            return Ok(ClusterGlyph::Pending(first(r)));
        }
        if let Some(result) = self.driver.prepare_now(r, p, e.covered == Some(true)) {
            // Negative validation is authoritative; a later source-choice redraw
            // can fall through to the next policy instead of retrying this face.
            let ink = result?;
            e.covered = Some(ink.is_some());
            self.facts.set(r, ink.is_some());
            return Ok(ink.unwrap_or_else(|| ClusterGlyph::Pending(first(r))));
        }
        // No frame waits for an unavailable owner or exhausted allowance. The
        // exact same placement goes to the worker without synchronous duplication.
        if self.driver.submit(Job {
            key: e.key.clone(),
            placement: Some(*p),
            validated: e.covered == Some(true),
        }) {
            e.ink.insert(size, None);
        }
        Ok(ClusterGlyph::Pending(first(r)))
    }
}
