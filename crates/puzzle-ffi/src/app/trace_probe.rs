//! Opt-in C-host diagnostics use tracing without a new logger dependency or export.
//!
//! C cannot install a Rust subscriber. PUZZLE_C_HOST_TRACE supplies a private
//! file before App creation; input observations are available in release hosts.
//! Debug fixtures additionally retain refresh, snapshot and geometry decisions.
//! Ordinary hosts install nothing and perform no diagnostic file I/O. Existing
//! caller logging wins, including a subscriber installed by a Rust embedding host.
//! The private input target may contain key/text facts; use only a private path.
//! No protocol grants or credentials enter this selected target set. Diagnostic
//! allocation/serialization costs are opt-in and not product frame scheduling.
use std::{fmt::Write as _, fs::File, io::Write as _, sync::Mutex};
use tracing::{
    Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

/// Installs the optional fixture subscriber; existing caller logging always wins.
pub(super) fn install() -> Result<(), crate::abi::ResultCode> {
    let Some(path) = std::env::var_os("PUZZLE_C_HOST_TRACE") else {
        return Ok(());
    };
    let file = File::options()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|_| crate::abi::ResultCode::Platform)?;
    // Later App creations in the same C fixture must not replace a caller's logger.
    let _ = tracing::subscriber::set_global_default(Probe(Mutex::new(file)));
    Ok(())
}

/// Diagnostic-only output serializes complete event lines across the native workers.
struct Probe(Mutex<File>);

/// Formats fields from the explicitly enabled private diagnostic targets.
struct Fields(String);
impl Visit for Fields {
    /// Input facts can be private; the caller deliberately opted into this sink.
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        // Writing to a String is infallible; formatting never calls back into Puzzle.
        let _ = write!(self.0, " {}={value:?}", field.name());
    }
}

impl Subscriber for Probe {
    /// Input is explicit/private; frame decisions remain a debug-fixture facility.
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        matches!(
            metadata.target(),
            "puzzle_desktop::input" | "puzzle_desktop::caret"
        ) || (cfg!(debug_assertions)
            && matches!(
                metadata.target(),
                "puzzle_desktop::refresh" | "puzzle_desktop::scheduling" | "puzzle_desktop::resize"
            ))
    }
    /// The selected diagnostic targets emit events only; no span storage is needed.
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    /// Unselected subsystem spans cannot add payloads to this event-only capture.
    fn record(&self, _: &Id, _: &Record<'_>) {}
    /// Scheduler receipts do not reconstruct causal span graphs.
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    /// Emits one complete receipt; a write error never becomes a native lifecycle error.
    fn event(&self, event: &tracing::Event<'_>) {
        if !self.enabled(event.metadata()) {
            return;
        }
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        // A file mutex belongs solely to this opt-in probe, never Session ownership.
        if let Ok(mut file) = self.0.lock() {
            let _ = writeln!(
                file,
                "{} {:?}{}",
                event.metadata().target(),
                event.metadata().level(),
                fields.0
            );
        }
    }
    /// Entering unrelated spans contributes no per-thread mutable state.
    fn enter(&self, _: &Id) {}
    /// Exiting unrelated spans likewise leaves the capture unchanged.
    fn exit(&self, _: &Id) {}
}

/// Release C hosts must observe input without enabling per-frame diagnostics.
#[cfg(test)]
mod tests;
