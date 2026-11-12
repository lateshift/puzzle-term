//! Diagnostic counters are compiled only into browser tests or the dev page.
//!
//! The frame hook follows a successful render_surface submission, so an rAF
//! callback with no damage cannot be counted as a presented frame. Output
//! counters observe actual bridge packets and the Session suffix separately.
//! A test may retain the submitted canvas in a 2D copy before WebGPU discards it.
//! This observer is absent even from the measurements development build.
//! It cannot manufacture damage, take a snapshot or submit a replacement image.
//! The development cadence page explicitly begins and ends an observation.
//! Its sample clock is the same runtime-relative clock used by the render hook.
//! A bare rAF callback never contributes a sample unless a GPU submission succeeds.
//! Observation allocates its storage before the interval and converts numbers afterward.
//! Lifecycle guards still reject disposed terminals, even in diagnostic builds.
#[cfg(feature = "measurements")]
use super::api::PuzzleTerminal;
use puzzle_term::bridge::checksum::{FNV1A_BASIS, fnv1a_update};
use std::time::Duration;
/// Actual retained glyph handles supply time-to-ink without extra draws.
mod ink;

/// One explicitly activated measurement belongs to a single terminal.
pub(super) struct Measurements {
    /// Disabled by default; ordinary tests do not hash all incoming output.
    pub(super) active: bool,
    /// Actual WebSocket payload bytes, excluding the protocol's type prefix.
    pub(super) received: u64,
    /// Position-sensitive checksum catches loss, duplication and reordering.
    pub(super) checksum: u64,
    /// Peak accepted output waiting in Session after a budgeted parse turn.
    pub(super) pending_peak: usize,
    /// Each entry is an actual successful GPU submission's terminal-relative time.
    pub(super) frames: Vec<Duration>,
    /// A diagnostic observation starts fresh together with its submission interval.
    pub(super) ink: ink::Observer,
    /// Callback-relative start, allowance, CPU, source, publication, deadline, end in ms.
    pub(super) turns: Vec<[f64; 7]>,
    /// Test-only readback copies the submitted canvas before browser buffer discard.
    #[cfg(test)]
    pub(super) capture: Option<(
        web_sys::HtmlCanvasElement,
        web_sys::CanvasRenderingContext2d,
    )>,
}

/// Creating a runtime adds no benchmark allocation until the probe activates it.
impl Default for Measurements {
    /// Inactive probes leave every production operation untouched apart from cfg hooks.
    fn default() -> Self {
        Self {
            active: false,
            received: 0,
            checksum: FNV1A_BASIS,
            pending_peak: 0,
            frames: Vec::new(),
            ink: Default::default(),
            turns: Vec::new(),
            #[cfg(test)]
            capture: None,
        }
    }
}

/// Activated probes observe the existing parser and rAF lanes without changing them.
impl Measurements {
    /// Counts wire bytes before parsing can retain a suffix or return ACK credit.
    pub(super) fn output(&mut self, bytes: &[u8], pending: usize) {
        if !self.active {
            return;
        }
        self.received += bytes.len() as u64;
        self.pending_peak = self.pending_peak.max(pending);
        // Hash the byte stream rather than a final grid that may discard old rows.
        self.checksum = fnv1a_update(self.checksum, bytes.iter().copied());
    }

    /// No submitted image means no frame event, even if the browser invoked rAF.
    pub(super) fn presented(&mut self, at: Duration) {
        if self.active {
            self.frames.push(at);
        }
    }
}

/// The dev page observes the ordinary runtime's submitted frames, not rAF requests.
#[cfg(feature = "measurements")]
#[wasm_bindgen::prelude::wasm_bindgen]
impl PuzzleTerminal {
    /// Begins a finite diagnostic interval; the normal package omits this binding.
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = measureScrollStart)]
    pub fn measure_scroll_start(&self) -> Result<f64, wasm_bindgen::JsValue> {
        let mut runtime = self.owner.borrow_mut();
        runtime.ensure_live()?;
        runtime.measurements.frames.clear();
        runtime.measurements.frames.reserve(512);
        runtime.measurements.active = true;
        // Frame timestamps use this same terminal-relative monotonic clock.
        Ok(runtime.started.elapsed().as_secs_f64() * 1000.0)
    }

    /// Ends observation and returns every actual submission time in milliseconds.
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = measureScrollFrames)]
    pub fn measure_scroll_frames(&self) -> Result<Vec<f64>, wasm_bindgen::JsValue> {
        let mut runtime = self.owner.borrow_mut();
        runtime.ensure_live()?;
        runtime.measurements.active = false;
        // Conversion is outside the scrolling interval; slow frames are retained.
        Ok(runtime
            .measurements
            .frames
            .iter()
            .map(|at| at.as_secs_f64() * 1000.0)
            .collect())
    }
}

/// Tests observe a real submission without building or requesting another frame.
#[cfg(test)]
impl Measurements {
    /// Copies only an installed probe's canvas, inside the actual runtime rAF.
    pub(super) fn capture_canvas(
        &self,
        canvas: &web_sys::HtmlCanvasElement,
    ) -> Result<(), wasm_bindgen::JsValue> {
        if let Some((copy, context)) = &self.capture {
            copy.set_width(canvas.width());
            copy.set_height(canvas.height());
            // The persistent 2D copy survives WebGPU drawing-buffer discard.
            context.draw_image_with_html_canvas_element(canvas, 0.0, 0.0)?;
        }
        Ok(())
    }
}
