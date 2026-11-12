//! The Chrome host yields between the shared native search-turn samples.
//! All elapsed CPU time comes from the neutral probe's actual Session frame.
//! Bindings exist only in the explicit measurement build; normal packages omit them.
//! Query setup is deliberately separate from its first timed slice.
//! The page owns each rAF wait, so completion cannot hide a synchronous scan.
//! Counts and progress getters observe state without examining additional rows.
use puzzle_term::test_support::search_probe::SearchProbe;
use wasm_bindgen::prelude::*;

/// Development-only owner of the cold 100,000-line query.
#[cfg_attr(feature = "measurements", wasm_bindgen)]
pub struct SearchBenchmark {
    /// The neutral fixture supplies the identical native/browser workload.
    probe: SearchProbe,
}

/// Separate steps let the page yield to genuine rAF rather than scan synchronously.
#[cfg_attr(feature = "measurements", wasm_bindgen)]
impl SearchBenchmark {
    /// Construction and query installation are excluded from slice timings.
    #[cfg_attr(feature = "measurements", wasm_bindgen(constructor))]
    pub fn new() -> Self {
        Self {
            probe: SearchProbe::new(),
        }
    }
    /// Returns CPU milliseconds and examined rows; hosts retain every sample.
    pub fn step(&mut self) -> Vec<f64> {
        let (millis, rows) = self.probe.step();
        // JS array conversion follows the timed frame, never inside it.
        vec![millis, rows as f64]
    }
    /// True only when the entire retained document has been searched.
    #[cfg_attr(feature = "measurements", wasm_bindgen(getter))]
    pub fn complete(&self) -> bool {
        self.probe.complete()
    }
    /// Must converge to the authored 1,000 hits, not an empty fast query.
    #[cfg_attr(feature = "measurements", wasm_bindgen(getter))]
    pub fn count(&self) -> usize {
        self.probe.count()
    }
}

impl Default for SearchBenchmark {
    /// A default still prepares the complete cold workload.
    fn default() -> Self {
        Self::new()
    }
}
