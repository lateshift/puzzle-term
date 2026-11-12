//! Publication predicts each kind from recent costs, independently of texture growth.
//!
//! Ownership is transferred before admission so the actual kind is known. One
//! retained unit stays ready when it cannot fit; no work is discarded or retried.
//! The host supplies refresh slack after ordinary frame work and D-33's margin.
use puzzle_text::{cluster_source::ClusterGlyph, glyph::GlyphPlacement, preparation::Completion};
use std::{collections::VecDeque, time::Duration};

/// Distinct costs must not lend an allocation spike to a cheap vector publication.
#[derive(Clone, Copy)]
pub(super) enum Kind {
    /// Already prepared vector geometry needs packing and cache insertion only.
    Vector,
    /// Device-size samples need a raster-atlas copy, never decoding here.
    Bitmap,
    /// Layered color graphs have independent clip/paint publication costs.
    Color,
    /// A mirror's first allocation or geometric growth is charged separately.
    Allocation,
}
/// One owned result can await a later reachable refresh without blocking cheaper lanes.
pub(super) enum Work {
    /// Loaded faces transfer converted geometry with their original style policy.
    Loaded(Completion),
    /// Platform sources transfer complete validated ink and its exact placement.
    Source(GlyphPlacement, ClusterGlyph),
}
impl Work {
    /// Classifies the indivisible operation before any atlas mutation.
    pub(super) fn kind(&self) -> Kind {
        use puzzle_text::preparation::Ink;
        match self {
            Self::Source(_, ClusterGlyph::Bitmap(_)) => Kind::Bitmap,
            Self::Source(_, ClusterGlyph::Color(_))
            | Self::Loaded(Completion {
                result: Ok(Some(Ink::Color(_))),
                ..
            }) => Kind::Color,
            _ => Kind::Vector,
        }
    }
}
/// Host allowance, bounded cost histories and one waiting unit per producer lane.
pub(super) struct Publication {
    /// Host-computed slack excludes the wake margin and ordinary remaining cost.
    pub(super) budget: Duration,
    /// Actual publication cost is removed from ordinary-frame prediction.
    pub(super) last: Duration,
    /// Recent samples per kind match D-33's bounded observation horizon.
    samples: [VecDeque<Duration>; 4],
    /// Each producer retains one result; a blocked lane cannot overwrite owned work.
    pub(super) waiting: [Option<Work>; 2],
    /// Alternation gives both loaded and platform result lanes a turn.
    pub(super) next_loaded: bool,
}
impl Publication {
    /// Recent p95 tolerates isolated noise without using the last or largest sample.
    pub(super) fn predicted(&self, kind: Kind) -> Duration {
        // Kinds never share a history: bitmap copies cannot set vector packing cost.
        let samples = &self.samples[kind as usize];
        // With no observation, the first indivisible unit teaches this kind's cost.
        if samples.is_empty() {
            return Duration::ZERO;
        }
        // A stack copy preserves chronological retirement without heap work per frame.
        let mut sorted = [Duration::ZERO; 32];
        let n = samples.len();
        for (dst, src) in sorted.iter_mut().zip(samples) {
            *dst = *src;
        }
        // Only populated samples enter the percentile; unused zeros cannot lower it.
        sorted[..n].sort_unstable();
        // Nearest-rank p95 uses the bounded recent history, never a lifetime maximum.
        sorted[(n * 95).div_ceil(100).saturating_sub(1)]
    }
    /// Growth observations belong only to allocation, never to ordinary packing.
    pub(super) fn record(&mut self, kind: Kind, cost: Duration) {
        let samples = &mut self.samples[kind as usize];
        // Retire oldest evidence so transient slow work cannot poison admission forever.
        if samples.len() == 32 {
            samples.pop_front();
        }
        // Successful publication contributes its measured cost after ownership transfer.
        samples.push_back(cost);
    }
    /// Retained ready ownership must request a frame even after application output stops.
    pub(super) fn ready(&self) -> bool {
        // Owned ready work keeps an idle terminal eligible for a completion wake.
        self.waiting.iter().any(Option::is_some)
    }
}
impl Default for Publication {
    /// Non-presenting callers start with half an emulated 60-Hz refresh.
    fn default() -> Self {
        Self {
            budget: Duration::from_nanos(1_000_000_000 / 120),
            last: Duration::ZERO,
            samples: Default::default(),
            waiting: Default::default(),
            next_loaded: false,
        }
    }
}

#[cfg(test)]
/// Growth outliers must never strand the inexpensive work of a different kind.
mod tests {
    use super::*;

    /// A first mirror allocation and a rare slow vector cannot set all future costs.
    #[test]
    fn expensive_kinds_and_outliers_do_not_strand_ready_vectors() {
        let mut publication = Publication::default();
        publication.record(Kind::Allocation, Duration::from_millis(20));
        publication.record(Kind::Bitmap, Duration::from_millis(12));
        for _ in 0..31 {
            publication.record(Kind::Vector, Duration::from_micros(5));
        }
        publication.record(Kind::Vector, Duration::from_millis(9));
        // A 100-us remaining refresh can still publish the ready vector while
        // both independently expensive work kinds wait for sufficient slack.
        let slack = Duration::from_micros(100);
        assert!(publication.predicted(Kind::Vector) < slack);
        assert!(publication.predicted(Kind::Allocation) > slack);
        assert!(publication.predicted(Kind::Bitmap) > slack);
        // New inexpensive observations retire old noise using the bounded horizon.
        for _ in 0..32 {
            publication.record(Kind::Bitmap, Duration::from_micros(25));
        }
        assert!(publication.predicted(Kind::Bitmap) < slack);
    }
}
