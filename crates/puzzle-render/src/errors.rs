//! Recoverable product GPU errors; strict observers keep validation a test failure.
//! The callback owns scalar context only, never a surface, frame or native window.
//! Frame work cannot hold this guard across a wgpu call, which may invoke it inline.
use std::sync::{Arc, Mutex};

/// Scalar evidence for the active submission, copied before GPU calls begin.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameContext {
    /// Host frame ordinal, independent of parser progress.
    pub number: u64,
    /// Published content extent in device pixels.
    pub published: [u32; 2],
    /// Acquired attachment extent, potentially different after a resize.
    pub acquired: [u32; 2],
}
/// Callback-visible error and context, separate from all GPU resource ownership.
#[derive(Default)]
struct State {
    /// Updated by the frame owner before encoding, never inside a GPU call.
    context: FrameContext,
    /// Latest failure retains its exact message until the owner drops that frame.
    error: Option<String>,
}
/// A notification owner whose presence replaces wgpu's default panic callback.
#[derive(Clone, Default)]
pub struct ErrorMonitor(Arc<Mutex<State>>);
impl ErrorMonitor {
    /// Installs recovery; `strict` is reserved for test/acceptance processes.
    /// A strict callback fails the test immediately instead of accepting repaired ink.
    pub fn install(
        device: &wgpu::Device,
        strict: bool,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        // Each device owns one callback; the monitor survives until its host consumes failures.
        let monitor = Self::default();
        let state = Arc::clone(&monitor.0);
        device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            // Preserve wgpu’s full validation path; truncation would lose the offending command.
            let message = error.to_string();
            let context = {
                // Release before tracing/waking: neither callback can borrow GPU owners.
                let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                state.error = Some(message.clone());
                state.context
            };
            tracing::error!(event="render.uncaptured_error", ?context, %message);
            wake();
            if strict {
                // Test hook: recovery must never conceal a validation regression (R14).
                panic!("GPU test validation failure: {message}");
            }
        }));
        monitor
    }
    /// Publishes scalar frame context without retaining a borrow of the frame.
    pub fn context(&self, context: FrameContext) {
        // A poisoned observer still carries recoverable evidence; it must not abort the product.
        self.0.lock().unwrap_or_else(|e| e.into_inner()).context = context;
    }
    /// Drops an erroneous frame by returning its failure to the resource owner.
    pub fn take_error(&self) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .error
            .take()
    }
}
