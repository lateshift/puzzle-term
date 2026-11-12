//! Choose one current result when a query first discovers matches (M12.search).
//! Later slices preserve it; only a new query or explicit navigation moves it.
//!
//! This is cold query setup over already discovered ranges, not another text scan.
//! Folded marker rows are not visible text. Choosing by physical display order
//! also avoids treating line IDs as coordinates after insertion or reflow.
//! A query with no results retains no selected index; its next slice tries again.
//! Once selected, the guard avoids repeating this choice as results accumulate.
//!
//! The caller supplies visibility policy without giving the matcher host resources.
//! A covered candidate is skipped before ranking; it can remain the fallback hit.
//! Later explicit navigation still uses the ordinary selected-result ordering.
use super::SearchModel;
use crate::{
    viewport::Viewport,
    vt::{Terminal, tracked::LineId},
};

impl SearchModel {
    /// Prefer the lowest visible match, otherwise return the first hit for centring.
    pub(crate) fn initial_navigation(
        &mut self,
        term: &Terminal,
        viewport: &Viewport,
        unobscured: impl Fn(usize, &crate::vt::search::FindMatch) -> bool,
    ) -> Option<LineId> {
        let engine = self.engine.as_mut()?;
        // Do not rerank an existing choice when a later slice reaches newer output.
        if engine.current().is_some() || engine.count() == 0 {
            return None;
        }
        // Retained identities, not numeric ID distances, map hits after row edits.
        let ring = &term.screen().ring;
        let mut visible = None;
        // Resolve only discovered hits, never scan terminal text or advance another slice.
        for index in 0..engine.count() {
            let Some(hit) = engine.resolve(term, index) else {
                continue;
            };
            let Some(source) = ring.index_of(hit.start.line) else {
                continue;
            };
            // Projection-aware rows exclude folded text; latest horizontal hit breaks ties.
            if let Some(row) = (0..term.size().1)
                .find(|row| !viewport.marker_row(*row) && viewport.source_row(*row) == Some(source))
            {
                if !unobscured(row, &hit) {
                    continue;
                }
                let candidate = (row, hit.start.column, index);
                if visible.is_none_or(|old| candidate > old) {
                    visible = Some(candidate);
                }
            }
        }
        // Discovery order is document order; index zero is the offscreen fallback.
        let index = visible.map_or(0, |(_, _, index)| index);
        let hit = engine.select(term, index)?;
        // Strong match fill and current counter must describe the same publication.
        self.refresh(term);
        // A visible hit must not shift the user's current view, even by a fraction.
        visible.is_none().then_some(hit.start.line)
    }
}
