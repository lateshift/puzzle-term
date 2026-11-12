//! Opt-in time-to-ink observations follow actual successful presentations.
//!
//! Logical row, column and source identity distinguish appearances from redraws.
//! A glyph leaving before completion is censored, never counted as fast ink.
//! Unsupported builtin tofu is excluded; provisional source choices count only
//! after successful validation. This observer prepares no substitute geometry.
use puzzle_term::{frame_builder::InkSample, text::glyph::GlyphKey};
use std::{collections::HashMap, time::Instant};

/// Copies diagnostic metadata while the builder still describes the submitted frame.
impl super::LiveWindow {
    /// Inspects final row handles without preparing or uploading replacement ink.
    pub fn observe_ink(&mut self, built: bool) {
        if self.text_context.completion_metrics().is_some() && built {
            // The count includes unsupported boxes; the observer excludes those by identity.
            let cells = self.builder.retained_ink_samples(&mut self.text_context);
            // Seal resolved contrast with these placements, before a newer slot arrives.
            self.ink_observer.built_zero_contrast.clear();
            self.ink_observer.built_zero_contrast.extend(
                cells
                    .iter()
                    .map(|cell| self.builder.zero_contrast(&self.snapshot, cell)),
            );
            self.glyph_status = Some([cells.len(), cells.iter().filter(|c| c.ready).count()]);
            self.ink_observer.built = cells;
        }
    }
}

/// One visible occurrence, independent of viewport scrolling and layer count.
type Identity = (u64, u32, GlyphKey);

/// Timing state for an occurrence retained on the visible screen.
struct Appearance {
    /// First successful presentation showing this neutral request.
    first: Instant,
    /// Latest successful presentation still containing the occurrence.
    frame: u64,
    /// A resolved occurrence is counted once, even across unchanged redraws.
    resolved: bool,
}

/// Development evidence retains every completed latency and censored count.
#[derive(Default)]
pub struct Observer {
    /// The producer handshake excludes the shell used to launch the workload.
    pub active: bool,
    /// Built requests are committed only if their surface presentation succeeds.
    pub built: Vec<InkSample>,
    /// Resolved color equality belongs to the built source boundary, not later VT output.
    pub built_zero_contrast: Vec<bool>,
    /// Neutral coordinates of the last submitted image; readback never captures new VT rows.
    pub presented_cells: Vec<[u32; 6]>,
    /// A parallel submitted contrast fact selects coverage-only diagnostic checks.
    pub presented_zero_contrast: Vec<bool>,
    /// Only currently visible appearances occupy this registry.
    visible: HashMap<Identity, Appearance>,
    /// Raw covered-glyph latencies include primary and fallback sources.
    samples: Vec<f64>,
    /// Every submitted frame retains its covered-cell missing-ink count (M16.2).
    missing_per_frame: Vec<usize>,
    /// Unfinished appearances leaving the screen are explicitly censored.
    censored: usize,
    /// Successful presentation ordinal stamps visible occurrences.
    pub frame: u64,
    /// First presentation of the workload, excluding setup.
    first: Option<Instant>,
    /// Last presentation that still showed unresolved eligible source requests.
    warming_end: Option<Instant>,
    /// All successful presentations in the workload's observation interval.
    frames: usize,
    /// Presentations from the first workload image through its last warming image.
    warming_frames: usize,
}

/// Appearance times come only from successful submissions, not parser arrivals.
impl Observer {
    /// Starts with fresh counters at the producer's readiness handshake.
    pub fn start(&mut self) {
        *self = Self {
            active: true,
            ..Self::default()
        };
    }

    /// Records submitted handles; failed or merely built frames cannot complete ink.
    pub fn presented(&mut self, now: Instant) {
        if !self.active {
            return;
        }
        // Advance only after successful surface presentation, never after a build.
        self.frame += 1;
        self.frames += 1;
        // The first workload image anchors warming cadence without shell setup.
        self.first.get_or_insert(now);
        // Reuse the coordinate allocation; a readback must describe this exact image.
        // Taking another VT snapshot here could consume damage from a newer frame.
        self.presented_cells.clear();
        self.presented_zero_contrast
            .clone_from(&self.built_zero_contrast);
        self.presented_cells.extend(self.built.iter().map(|sample| {
            [
                sample.row,
                sample.column,
                sample.span,
                // Readiness accompanies the same coordinates and source identity.
                u32::from(sample.ready),
                sample.key.face,
                // Retain the identity needed to diagnose a blank or stale source crop.
                sample.key.glyph_id,
            ]
        }));
        // Retain all frames; a later resolved frame cannot hide an earlier provisional box.
        self.missing_per_frame.push(
            self.built
                .iter()
                .filter(|sample| {
                    sample.key.face != puzzle_term::text::builtin::TOFU_FACE && !sample.ready
                })
                .count(),
        );
        // Coordinates survive this drain so later GPU sampling needs no new layout.
        for sample in self.built.drain(..) {
            // Truly unsupported scalars retain their labelled box by specification.
            if sample.key.face == puzzle_term::text::builtin::TOFU_FACE {
                continue;
            }
            // Absolute row identity distinguishes scrolling from a new occurrence.
            let id = (sample.line.0, sample.column, sample.key);
            // An unchanged redraw keeps its first appearance time.
            let entry = self.visible.entry(id).or_insert(Appearance {
                first: now,
                frame: self.frame,
                resolved: false,
            });
            // The stamp lets retirement detect precisely which occurrences disappeared.
            entry.frame = self.frame;
            // Count a ready occurrence once, even if it remains visible for minutes.
            if sample.ready && !entry.resolved {
                self.samples
                    .push(now.duration_since(entry.first).as_secs_f64() * 1000.0);
                // Later presentations cannot duplicate this latency sample.
                entry.resolved = true;
            }
        }
        // A removed unresolved glyph supplies no positive latency observation.
        let frame = self.frame;
        let censored = &mut self.censored;
        self.visible.retain(|_, entry| {
            if entry.frame == frame {
                return true;
            }
            *censored += usize::from(!entry.resolved);
            false
        });
        // Only genuinely unresolved visible ink extends the warming interval.
        if self.visible.values().any(|entry| !entry.resolved) {
            self.warming_end = Some(now);
            self.warming_frames = self.frames;
        }
    }

    /// Produces exact max/p95 over every completed occurrence, outside measured frames.
    pub fn report(&self, elapsed: f64) -> serde_json::Value {
        let mut samples = self.samples.clone();
        // Percentiles use every completed sample, including the slowest one.
        samples.sort_unstable_by(f64::total_cmp);
        let p95 = samples.get((samples.len() * 95).div_ceil(100).saturating_sub(1));
        // Idle damage-driven screens intentionally present fewer than refresh-rate frames.
        let warming_seconds = self
            .first
            .zip(self.warming_end)
            .map(|(first, end)| end.duration_since(first).as_secs_f64());
        serde_json::json!({
            // Keep the raw ordinal series; zero at the end cannot certify earlier frames.
            "missing_per_frame": self.missing_per_frame,
            "covered_appearances": samples.len(), "latency_ms": self.samples,
            "max_ms": samples.last(), "p95_ms": p95, "censored": self.censored,
            "unresolved_visible": self.visible.values().filter(|e| !e.resolved).count(),
            "presentations": self.frames, "whole_interval_fps": self.frames as f64 / elapsed,
            "warming_seconds": warming_seconds, "warming_presentations": self.warming_frames,
            "warming_fps": warming_seconds.filter(|s| *s > 0.0)
                .map(|s| self.warming_frames.saturating_sub(1) as f64 / s),
        })
    }
}
/// Evidence guards prevent redraws and removed provisional glyphs skewing percentiles.
#[cfg(test)]
mod tests;
