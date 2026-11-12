//! Bundled and user-loaded faces obey the same cold-work admission as fallback.
//!
//! Atlas hits bypass this module. A cold key either receives owned prepared
//! geometry immediately or owns one accepted background job. Ready geometry
//! transfers to the atlas even if its original row has already disappeared.
use super::FontSet;
use crate::{
    glyph::GlyphPlacement,
    preparation::{Completion, Driver, Ink, Job, Outline, Pending},
};

impl FontSet {
    /// Installs a host-owned scheduler after font loading, without starting font work.
    pub fn set_preparation_driver(&mut self, driver: Box<dyn Driver>) {
        // The driver owns platform scheduling; the font set owns key deduplication.
        self.preparation = Some(Pending {
            // Confirmed empty contextual components do not need another job.
            empty: Default::default(),
            // Accepted jobs stay here until their completed ownership is transferred.
            keys: Default::default(),
            driver,
        });
    }
    /// Returns real prepared ink, or None while a background job owns this key.
    pub fn prepare_outline(
        &mut self,
        placement: &GlyphPlacement,
        style: [bool; 3],
    ) -> Option<Result<Option<Outline>, String>> {
        use crate::glyph::GlyphOutlineSource;
        let Some(pending) = self.preparation.as_mut() else {
            // Headless probes use their explicit synchronous clock and original source seam.
            return Some(self.outline(placement.key).and_then(Outline::new));
        };
        // Empty ink is a final result, distinct from unresolved provisional glyphs.
        if pending.empty.contains(&placement.key) {
            return Some(Ok(None));
        }
        // A queued key is already owned by the worker and must not be submitted twice.
        if pending.keys.contains(&placement.key) {
            return None;
        }
        // Procedural keys need no font bytes; ordinary keys require a retained face.
        let face = self.faces.get(placement.key.face as usize);
        // Only the documented builtin namespaces can bypass loaded face lookup.
        if face.is_none()
            && placement.key.face != crate::builtin::BUILTIN_FACE
            && placement.key.face != crate::builtin::TOFU_FACE
            && placement.key.face != crate::builtin::TOFU_COMPONENT_FACE
        {
            return Some(Err("glyph face absent".into()));
        }
        // Cold work carries a placement copy so completion survives row replacement.
        let job = Job {
            placement: *placement,
            // Arc-backed bytes preserve the mapping while a worker parses this face.
            data: face.map(|f| f.data()),
            // The variable weight joins the worker's parsed-face identity.
            weight: face.map_or(400, |f| f.weight()),
            style,
        };
        // No discarded validation is performed before the host classifies this job.
        // A ready parsed face may finish under the host's measured refresh allowance.
        if let Some(ink) = pending.driver.prepare_now(&job) {
            return Some(ink.and_then(|ink| match ink {
                // Validated geometry transfers directly to drawing instead of a side cache.
                Some(Ink::Outline(outline)) => Ok(Some(outline)),
                None => Ok(None),
                Some(Ink::Color(_)) => Err("color traversal must remain on the worker".into()),
            }));
        }
        // A full bounded lane declines ownership; the next row build can submit again.
        if pending.driver.submit(job) {
            // Only successful handoff deduplicates future draws of this key.
            pending.keys.insert(placement.key);
        }
        None
    }
    /// Moves one completed loaded-face shape to boundary publication.
    pub fn take_preparation_ready(&mut self) -> Option<Completion> {
        // Absence of a platform scheduler means there is no asynchronous ownership.
        let pending = self.preparation.as_mut()?;
        // Taking the completion releases one bounded lane slot without discarding ink.
        let done = pending.driver.poll()?;
        // Publication now owns the result, so another worker job is unnecessary.
        pending.keys.remove(&done.placement.key);
        // Retain only the empty fact; real geometry moves into the ordinary glyph store.
        if matches!(done.result, Ok(None)) {
            pending.empty.insert(done.placement.key);
        }
        Some(done)
    }
}

impl FontSet {
    /// A live host routes all cold color traversal through the loaded-face worker.
    pub fn preparation_scheduled(&self) -> bool {
        // Color callers use this to keep unpredictable graph traversal off live frames.
        self.preparation.is_some()
    }
}
