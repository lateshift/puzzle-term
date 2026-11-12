//! Compare live source intervals using maintained order, never numeric LineIds.
//! Region edits reorder identities; optional circular coordinates follow those
//! handle moves. Subtract the current oldest row coordinate before comparing;
//! this unwraps deque coordinates without a half-u32-space assumption.
//!
//! The comparison borrows only fixed watcher metadata and its two live tracker
//! receipts. It does not resolve row identities with index_of, walk history or
//! inspect text. Source mutation is sticky: a later equal-looking redraw cannot
//! rehabilitate a changed range and thereby authorize a stale relationship.
//! Half-open gaps matter: ranges sharing only an excluded end are disjoint.
use super::WriteClock;
use crate::{RangeWatch, TrackedPositions};
use std::cmp::Ordering;

/// Relationship of two half-open tracked source intervals on one screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangeRelation {
    /// Neither includes any source from the other, including touching gaps.
    Disjoint,
    /// Start and end gaps are equal.
    Equal,
    /// The first interval includes all of the second.
    Contains,
    /// The first interval is entirely inside the second.
    Within,
    /// The ranges overlap without containment.
    Crossing,
}

/// Current tracker state and maintained row order jointly authorize comparison.
impl WriteClock {
    /// Retained row indices come from the same maintained order as relationship
    /// checks. This is metadata-only: it neither scans rows nor calls index_of.
    pub(crate) fn order(
        &self,
        watch: RangeWatch,
        tracked: &TrackedPositions,
        origin: u32,
    ) -> Option<(usize, usize)> {
        // Watch identity belongs to this terminal; a released or foreign token
        // cannot borrow order from another live entry with similar endpoints.
        let entry = self
            .watches
            .as_ref()?
            .entries
            .iter()
            .find(|e| e.id == watch)?;
        // Sticky mutation must be tested before exposing a navigation target.
        // Equal final text cannot rehabilitate an identical inside rewrite.
        if entry.state(tracked)?.changed {
            return None;
        }
        // Circular order coordinates unwrap relative to the current oldest row;
        // subtraction is deque arithmetic, not a wrapping content/write clock.
        Some((
            entry.orders.0.wrapping_sub(origin) as usize,
            entry.orders.1.wrapping_sub(origin) as usize,
        ))
    }
    /// Changed, foreign-screen or evicted source cannot authorize a relationship.
    pub(crate) fn relation(
        &self,
        first: RangeWatch,
        second: RangeWatch,
        tracked: &TrackedPositions,
        origin: u32,
    ) -> Option<RangeRelation> {
        let entries = &self.watches.as_ref()?.entries;
        let a = entries.iter().find(|e| e.id == first)?;
        let b = entries.iter().find(|e| e.id == second)?;
        let sa = a.state(tracked)?;
        let sb = b.state(tracked)?;
        if sa.changed || sb.changed || a.alternate != b.alternate {
            return None;
        }
        // Both endpoints come from maintained, same-document watch metadata.
        // No row lookup, cell visit or producer-supplied coordinate occurs here.
        let a0 = (a.orders.0.wrapping_sub(origin), sa.start.1);
        let a1 = (a.orders.1.wrapping_sub(origin), sa.end.1);
        let b0 = (b.orders.0.wrapping_sub(origin), sb.start.1);
        let b1 = (b.orders.1.wrapping_sub(origin), sb.end.1);
        // Containment uses both ends; comparing starts alone would accept
        // crossing siblings or mistake a touching endpoint for shared text.
        let starts = compare(a0, b0);
        let ends = compare(a1, b1);
        Some(
            if compare(a1, b0) != Ordering::Greater || compare(b1, a0) != Ordering::Greater {
                RangeRelation::Disjoint
            } else if starts == Ordering::Equal && ends == Ordering::Equal {
                RangeRelation::Equal
            } else if starts != Ordering::Greater && ends != Ordering::Less {
                RangeRelation::Contains
            } else if starts != Ordering::Less && ends != Ordering::Greater {
                RangeRelation::Within
            } else {
                RangeRelation::Crossing
            },
        )
    }
}

/// Normalized document coordinates have lexicographic row/column order.
fn compare(a: (u32, usize), b: (u32, usize)) -> Ordering {
    a.cmp(&b)
}
