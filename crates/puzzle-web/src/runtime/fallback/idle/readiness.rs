//! Canvas readiness is learned from completed background draws, never font names.
//!
//! Canvas does not reveal the resolved physical fallback face. Each source owns
//! its immutable CSS family order; style, presentation and script keep unrelated
//! fallback families from inheriting readiness. A larger pixel stage must first
//! finish off-frame. The rolling cost window matches D-33's 32 measured turns.
use puzzle_term::text::{cluster_source::ClusterRequest, glyph::GlyphPlacement};
use std::time::Duration;

/// Completed calls retain bounded percentile evidence for one font policy.
#[derive(Default)]
pub(super) struct Readiness {
    /// At most 256 script/style policies survive; oldest use retires first.
    entries: Vec<Entry>,
    /// A cold policy needs one readiness observation before more clusters choose a lane.
    discovering: Vec<Key>,
}
/// The CSS family stack belongs to the source; this key completes its policy.
type Key = (
    puzzle_term::text::font_set::FontStyle,
    puzzle_term::text::presentation::Presentation,
    [u8; 4],
);
/// A completed background draw grants readiness at its tested device geometry.
struct Entry {
    /// Script disambiguates opaque physical fallback faces within one CSS stack.
    key: Key,
    /// A large first decode/readback cannot borrow a smaller stage's readiness.
    area: u64,
    /// A rejected or over-budget call revokes readiness until background success.
    ready: bool,
    /// Thirty-two recent complete calls bound prediction storage like D-33.
    samples: std::collections::VecDeque<Duration>,
}
impl Readiness {
    /// Defers lane selection while a family's first real draw is already underway.
    pub(super) fn reserve(&mut self, r: ClusterRequest<'_>, p: &GlyphPlacement) -> bool {
        // An established policy can immediately choose its ordinary admission lane.
        if self.costs(r, p).is_some() {
            return true;
        }
        // Reservation identity shares the same style/script policy as readiness.
        let k = key(r);
        // One first draw teaches the family; duplicate cold policy jobs add no evidence.
        if self.discovering.contains(&k) {
            return false;
        }
        // No job has been prepared or discarded: other clusters retry after the
        // readiness completion wakes their rows, then choose inline or background.
        self.discovering.push(k);
        true
    }
    /// A full queue relinquishes the first-draw reservation because it owns no job.
    pub(super) fn cancel(&mut self, r: ClusterRequest<'_>) {
        // Refused ownership must not leave the family permanently reserved.
        self.discovering.retain(|k| *k != key(r));
    }
    /// Returns median and p95 of completed calls only after worker readiness.
    pub(super) fn costs(&self, r: ClusterRequest<'_>, p: &GlyphPlacement) -> Option<[Duration; 2]> {
        // No completed observation means the opaque Canvas face is still cold.
        let entry = self.entries.iter().find(|e| e.key == key(r))?;
        // A revoked or larger untested device stage always returns to background work.
        if !entry.ready || area(p) > entry.area || entry.samples.is_empty() {
            return None;
        }
        // Percentile calculation uses bounded stack storage on the rAF path.
        let mut sorted = [Duration::ZERO; 32];
        let n = entry.samples.len();
        for (dst, src) in sorted.iter_mut().zip(&entry.samples) {
            *dst = *src;
        }
        // Ignore unused slots so early readiness cannot inherit artificial zero costs.
        sorted[..n].sort_unstable();
        Some([
            // The median describes typical cost; p95 is the conservative admission estimate.
            sorted[n / 2],
            sorted[(n * 95).div_ceil(100).saturating_sub(1)],
        ])
    }
    /// Only successful worker ink establishes readiness; inline overruns revoke it.
    pub(super) fn record(
        &mut self,
        r: ClusterRequest<'_>,
        p: &GlyphPlacement,
        cost: Duration,
        worker: bool,
        success: bool,
        overrun: bool,
    ) {
        let key = key(r);
        // Only a background completion can release an outstanding discovery reservation.
        if worker {
            self.discovering.retain(|k| *k != key);
        }
        let mut entry = if let Some(i) = self.entries.iter().position(|e| e.key == key) {
            // Moving an observed policy to the end also updates its eviction recency.
            self.entries.remove(i)
        } else {
            Entry {
                key,
                // A new entry grants no tested device-size coverage until a successful draw.
                area: 0,
                ready: false,
                samples: Default::default(),
            }
        };
        // A failed background draw cannot certify an available fallback font.
        if worker && success {
            entry.ready = true;
            // Previously tested smaller geometry remains eligible after a larger draw.
            entry.area = entry.area.max(area(p));
        }
        // Unpredictable inline cost revokes permission before the next cluster arrives.
        if !success || overrun {
            entry.ready = false;
        }
        // Old slow calls age out without changing a deadline or granting a fixed quantum.
        if entry.samples.len() == 32 {
            entry.samples.pop_front();
        }
        entry.samples.push_back(cost);
        self.entries.push(entry);
        // Evict the least recently observed policy; scalar coverage facts stay independent.
        if self.entries.len() > 256 {
            self.entries.remove(0);
        }
    }
}
/// Readiness crosses neither style selection nor an untested fallback script.
fn key(r: ClusterRequest<'_>) -> Key {
    (
        r.style,
        r.presentation,
        puzzle_term::text::cluster_font::script(r.cluster),
    )
}
/// Width and height are bounded by the raster page before any Canvas allocation.
fn area(p: &GlyphPlacement) -> u64 {
    u64::from(p.ink_limit_px) * u64::from(p.cell_height_px)
}
