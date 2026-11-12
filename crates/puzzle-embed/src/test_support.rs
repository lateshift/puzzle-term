//! Shared functional-test hang guard, independent of performance acceptance.
//!
//! Parallel debug GPU/PTY work can take tens of seconds on a busy machine.
//! Five minutes catches a lost wake or deadlock without asserting debug speed.
//! Release witnesses separately enforce every existing millisecond budget.

/// A deadlock guard, never a latency or throughput budget.
pub(crate) const HANG_GUARD: std::time::Duration = std::time::Duration::from_secs(300);

/// Emits unchanged budget receipts through the library's only logging API (R10).
pub(crate) fn report(message: std::fmt::Arguments<'_>) {
    // A thread-local subscriber avoids global initialization races in parallel tests.
    // Removing prefixes retains the measurement wire spelling consumed by xtask.
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_target(false)
        .with_level(false)
        .with_ansi(false)
        .with_writer(std::io::stdout)
        .finish();
    tracing::subscriber::with_default(subscriber, || tracing::info!("{message}"));
}
