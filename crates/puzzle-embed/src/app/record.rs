//! Startup metadata for a private native action recording (§13.4).
//!
//! This header is written before the PTY drain starts. It fixes the grid,
//! DPR and the resolved font size that determine row layout in replay.

use crate::recording::{Recorder, RecordingReader};
use puzzle_term::platform::PtySize;
use serde_json::json;
use std::io::Read;

/// Wraps the OS read call only for an explicitly requested recording.
pub(super) fn reader(
    inner: Box<dyn Read + Send>,
    recorder: Option<&Recorder>,
) -> Box<dyn Read + Send> {
    // The tap sits before the fixed pool and records true OS read
    // boundaries. The ordinary path keeps the original boxed reader.
    if let Some(recorder) = recorder {
        Box::new(RecordingReader::new(inner, recorder.clone()))
    } else {
        inner
    }
}

/// Records the runtime geometry and source revision ahead of all PTY bytes.
pub(super) fn header(
    recorder: Option<&Recorder>,
    grid: PtySize,
    dpr: f32,
    font_size: f32,
    commit: &str,
) -> Result<(), String> {
    let Some(recorder) = recorder else {
        // A normal window does not execute Git or create recording JSON.
        return Ok(());
    };
    // Both cells and device scale are needed: a byte-identical VT screen
    // can use different hinted outlines on a 1× and a 2× display.
    // The effective size belongs in the header because config.json or
    // --set may override the run-only 15 px fallback before the PTY starts.
    // No config path or user profile name enters the private recording:
    // geometry is enough to reproduce the drawn cell positions.
    let data = json!({"grid": [grid.columns, grid.rows],
        "dpr": dpr, "font": {"family": "JetBrains Mono", "size": font_size,
        "weight": 400}, "settings": {"source": "M6 layered config"},
        "commit": commit});
    recorder.emit("header", data)
}

/// Records immutable creation options before a child can produce its first read.
pub(super) fn surface(
    options: &super::SurfaceOptions,
    grid: puzzle_term::platform::PtySize,
    info: crate::render_thread::surface::SurfaceInfo,
    settings: &crate::settings::DesktopSettings,
) -> Result<(), String> {
    // The actor supplied the same metrics used for the initial PTY dimensions.
    // Record a host-provided revision; library creation must never invoke Git.
    header(
        options.recorder(),
        grid,
        info.scale as f32,
        settings.run_font_size(),
        options.revision(),
    )
}
