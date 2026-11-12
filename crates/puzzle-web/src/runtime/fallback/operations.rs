//! Draw/readback counters separate source work from event-loop queue residence.
//!
//! Counters contain Rust values only, so the source remains Send. Each call
//! records its synchronous API duration; browser-internal font loading remains
//! inside those durations rather than being reported as a fictitious zero.
//!
//! Draw and readback each retain count and duration, not one combined sample.
//! This distinguishes repeated validation from expensive synchronization.
//! Readback timing includes copying returned ImageData into owned Rust bytes.
//! Surface creation is counted once for the shared resource's lazy canvas.
//! Scalar validation and marker warmup are distinct additional call causes.
//! A caller snapshots totals around one preparation to attribute its own work.
//! Failed API attempts are counted too; success does not hide their cost.
//! These fixed-size values never retain a bitmap or JavaScript object.
use super::{OffscreenCanvasRenderingContext2d, js_error};
use std::cell::Cell;

/// Draw/readback counts and times, surfaces, scalar probes and marker warmups.
#[derive(Default)]
pub(super) struct Counts(pub(super) Cell<[u64; 7]>);
impl Counts {
    /// Extra validation and marker work stay attributable to their containing job.
    pub(super) fn validation(&self, marker: bool) {
        let mut n = self.0.get();
        // Counters distinguish warmup from probes that the scalar cache suppresses.
        n[if marker { 6 } else { 5 }] += 1;
        self.0.set(n);
    }
    /// Records creation separately from work performed on an existing context.
    pub(super) fn created(&self) {
        let mut n = self.0.get();
        n[4] += 1;
        self.0.set(n);
    }
    /// A successful or failed fillText is one measured Canvas API call.
    pub(super) fn draw(
        &self,
        canvas: &OffscreenCanvasRenderingContext2d,
        text: &str,
        x: f64,
        y: f64,
    ) -> Result<(), String> {
        let at = web_time::Instant::now();
        let result = canvas.fill_text(text, x, y).map_err(js_error);
        self.record(0, at.elapsed());
        result
    }
    /// Readback owns its returned bytes and exposes GPU synchronization cost.
    pub(super) fn read(
        &self,
        canvas: &OffscreenCanvasRenderingContext2d,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, String> {
        let at = web_time::Instant::now();
        let result = canvas
            .get_image_data(0.0, 0.0, f64::from(width), f64::from(height))
            .map_err(js_error)
            .map(|data| data.data().0);
        self.record(2, at.elapsed());
        result
    }
    /// Count attempts, including failures, without retaining any browser object.
    fn record(&self, offset: usize, elapsed: std::time::Duration) {
        let mut n = self.0.get();
        n[offset] += 1;
        n[offset + 1] += elapsed.as_nanos().min(u128::from(u64::MAX)) as u64;
        self.0.set(n);
    }
}
