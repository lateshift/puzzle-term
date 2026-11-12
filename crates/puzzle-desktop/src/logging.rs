//! Native tracing filters and the optional second, rotated file sink (§5.6).
//!
//! Console records always use stderr so stdout remains available for CLI
//! results. A file is opened before installing the subscriber; an open
//! failure leaves the console active and emits one stable warning event.
//! Both layers receive the same Targets filter, including privacy-sensitive
//! input diagnostics only when the user explicitly enables that debug target.

use std::{path::Path, str::FromStr};
use tracing_subscriber::{filter::Targets, prelude::*};
/// Bounded, line-flushed native file output with one previous-run copy.
mod file;

/// Parses the native override or selects the default info-level diagnostics.
pub(crate) fn targets() -> Result<Targets, String> {
    let spec = std::env::var("PUZZLE_LOG").unwrap_or_else(|_| "info".into());
    // Reject an invalid explicit filter so a developer notices a typo
    // instead of silently missing diagnostics during a harness run.
    Targets::from_str(&spec).map_err(|error| format!("PUZZLE_LOG: {error}"))
}

/// Installs stderr diagnostics and an optional file selected by CLI or env.
pub(crate) fn init(path: Option<&str>) -> Result<(), String> {
    let filter = targets()?;
    // The console writer is named explicitly: tracing-subscriber's fmt
    // default uses stdout, which would contaminate CLI result streams.
    let console = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(filter.clone());
    // CLI overrides the environment so a one-off diagnosis does not write
    // into the user's customary log path. M6.5 adds the settings fallback.
    let chosen = path
        .map(str::to_owned)
        .or_else(|| std::env::var("PUZZLE_LOG_FILE").ok());
    let (sink, error) = match chosen.as_deref() {
        Some(path) if !path.is_empty() => match file::FileSink::open(Path::new(path)) {
            Ok(sink) => (Some(sink), None),
            Err(error) => (None, Some(error.to_string())),
        },
        _ => (None, None),
    };
    let file = sink.map(|sink| {
        // SystemTime already formats UTC with six fractional digits; no
        // local timezone or additional dependency belongs in this layer.
        tracing_subscriber::fmt::layer()
            .with_writer(sink)
            .with_ansi(false)
            .with_timer(tracing_subscriber::fmt::time::SystemTime)
            .with_filter(filter)
    });
    tracing_subscriber::registry()
        .with(console)
        .with(file)
        .init();
    if let Some(error) = error {
        tracing::warn!(event = "desktop.log_file_open_failed", %error, "continuing without file log");
    }
    Ok(())
}
