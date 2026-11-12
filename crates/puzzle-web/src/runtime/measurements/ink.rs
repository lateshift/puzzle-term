//! Time-to-ink follows retained handles of actual submitted browser frames.
//!
//! This opt-in observer never requests a frame or prepares a substitute glyph.
//! Departing provisional appearances are censored; unsupported tofu is excluded.
use puzzle_term::{frame_builder::InkSample, text::glyph::GlyphKey};
use std::{collections::HashMap, time::Duration};

/// Row identity, column and source key distinguish one visible occurrence.
type Identity = (u64, u32, GlyphKey);
/// An occurrence persists until resolved ink is submitted or it leaves the screen.
struct Appearance {
    /// Successful presentation when this glyph first appeared.
    first: Duration,
    /// Wall clock correlates this occurrence with source queue/draw/delivery events.
    first_wall: f64,
    /// First and last presentation ordinal distinguish redraws from new text.
    frames: [u64; 2],
    /// Each covered occurrence contributes exactly one time-to-ink sample.
    resolved: bool,
}
/// All samples belong to one explicitly enabled finite workload observation.
#[derive(Default)]
pub(in crate::runtime) struct Observer {
    /// Only currently visible occurrences occupy this bounded registry.
    visible: HashMap<Identity, Appearance>,
    /// Every resolved latency retains both milliseconds and refresh-count delay.
    pub(in crate::runtime) samples: Vec<(f64, u64)>,
    /// Latency, face, glyph, instance, first and final browser wall-clock times.
    pub(in crate::runtime) details: Vec<[f64; 6]>,
    /// Departing unresolved glyphs cannot be reported as fast completion.
    pub(in crate::runtime) censored: usize,
    /// Only successful GPU submissions advance this ordinal.
    frame: u64,
}
impl Observer {
    /// Commits the retained row handles only after render_surface succeeds.
    pub(in crate::runtime) fn presented(&mut self, at: Duration, cells: Vec<InkSample>) {
        // Failed surface acquisitions cannot advance the observed presentation ordinal.
        self.frame += 1;
        for sample in cells {
            // Permanent unsupported diagnostics are outside covered-glyph latency.
            if sample.key.face == puzzle_term::text::builtin::TOFU_FACE {
                continue;
            }
            // A reused cell in a new logical row is a distinct visible appearance.
            let id = (sample.line.0, sample.column, sample.key);
            let e = self.visible.entry(id).or_insert(Appearance {
                first: at,
                first_wall: js_sys::Date::now(),
                frames: [self.frame; 2],
                resolved: false,
            });
            // A latest-frame stamp identifies disappeared occurrences after scrolling.
            e.frames[1] = self.frame;
            if sample.ready && !e.resolved {
                self.samples.push((
                    (at - e.first).as_secs_f64() * 1000.0,
                    self.frame - e.frames[0],
                ));
                // Identity joins this occurrence to its source queue and Canvas call.
                // The wall epoch is independent of the terminal construction time.
                self.details.push([
                    (at - e.first).as_secs_f64() * 1000.0,
                    f64::from(sample.key.face),
                    f64::from(sample.key.glyph_id),
                    f64::from(sample.key.instance),
                    e.first_wall,
                    js_sys::Date::now(),
                ]);
                // A retained ready cell contributes once, not on every unchanged redraw.
                e.resolved = true;
            }
        }
        // No positive sample is fabricated when scrolling removes pending ink.
        let frame = self.frame;
        let censored = &mut self.censored;
        self.visible.retain(|_, e| {
            if e.frames[1] == frame {
                return true;
            }
            *censored += usize::from(!e.resolved);
            false
        });
    }
    /// A final unresolved count distinguishes incomplete workloads from censored text.
    #[cfg(test)]
    pub(in crate::runtime) fn unresolved(&self) -> usize {
        self.visible.values().filter(|e| !e.resolved).count()
    }
}
