//! Harness-only PTY probes use the embedding-owned transport.
pub(crate) use puzzle_embed::diagnostics::pty::*;
/// Headless process-to-Session exercise for the M6.4 gate.
mod smoke;
pub(crate) use smoke::run as smoke;
/// Real PTY exit policy checks that do not need an OS window.
mod exit_probe;
pub(crate) use exit_probe::run as exit_probe;
