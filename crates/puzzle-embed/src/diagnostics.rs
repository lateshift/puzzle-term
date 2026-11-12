//! Developer diagnostics for the native regression harness, not the host API.
//!
//! This opt-in feature preserves existing probes during the ownership move.
//! Native hosts use App/Surface and their root-exported payloads. These owners
//! may change with tests and are not a second supported embedding interface.
// Keep extra protocol diagnostics behind a whole-file boundary.
include!("diagnostics_optional.rs");
/// Harness-only bitmap access; not native-host API.
pub mod bitmap {
    pub use crate::bitmap::*;
}
/// Harness-only block on access; not native-host API.
pub mod block_on {
    pub use crate::block_on::*;
}
/// Harness-only drawing access; not native-host API.
pub mod drawing {
    pub use crate::drawing::*;
}
/// Harness-only font data access; not native-host API.
pub mod font_data {
    pub use crate::font_data::*;
}
/// Harness-only gpu test access; not native-host API.
#[cfg(any(test, feature = "test-support"))]
pub mod gpu_test {
    pub use crate::gpu_test::*;
}
/// Harness-only grid rasterizer access; not native-host API.
pub mod grid_rasterizer {
    pub use crate::grid_rasterizer::*;
}
/// Harness-only keyboard access; not native-host API.
pub mod keyboard {
    pub use crate::keyboard::*;
}
/// Harness-only pipeline access; not native-host API.
pub mod pipeline {
    pub use crate::pipeline::*;
}
/// Harness-only pipeline metrics access; not native-host API.
#[cfg(any(debug_assertions, feature = "lock-metrics"))]
pub mod pipeline_metrics {
    pub use crate::pipeline_metrics::*;
}
/// Harness-only png access; not native-host API.
pub mod png {
    pub use crate::png::*;
}
/// Harness-only pty access; not native-host API.
pub mod pty {
    pub use crate::pty::*;
}
/// Harness-only pty pool access; not native-host API.
pub mod pty_pool {
    pub use crate::pty_pool::*;
}
/// Harness-only recording access; not native-host API.
pub mod recording {
    pub use crate::recording::*;
}
/// Harness-only render thread access; not native-host API.
pub mod render_thread {
    pub use crate::render_thread::*;
}
/// Harness-only resize timing access; not native-host API.
pub mod resize_timing {
    pub use crate::resize_timing::*;
}
/// Harness-only session thread access; not native-host API.
pub mod session_thread {
    pub use crate::session_thread::*;
}
/// Harness-only system fonts access; not native-host API.
pub mod system_fonts {
    pub use crate::system_fonts::*;
}
/// Harness-only thread cpu access; not native-host API.
pub mod thread_cpu {
    pub use crate::thread_cpu::*;
}
/// Harness-only window access; not native-host API.
pub mod window {
    pub use crate::window::*;
}
/// Harness-only settings access; not native-host API.
pub mod settings {
    pub use crate::settings::*;
}

/// Harness-only native codec entry point; not part of the native-host API.
/// Scripted image scenes complete the same bounded decode request as the App worker.
pub fn decode_image(
    request: puzzle_term::images::DecodeRequest,
) -> puzzle_term::images::DecodeResult {
    crate::image_decode::decode(request)
}
