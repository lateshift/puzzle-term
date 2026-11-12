//! Browser-side flow control is independent of WebSocket event timing (§12.3).
//!
//! Input credit is spent before a message reaches the socket; a large paste
//! therefore stays in application order without monopolizing TCP and hiding
//! output ACKs. Output credit is returned only after Session consumed bytes,
//! including bytes consumed by a later budget continuation or frame turn.
/// FIFO paste chunks spend only credit granted by the development bridge.
pub mod credit;
/// Output provenance separates bridge credit from host write completion.
pub mod parsed;

/// Browser builds use the core codec; native ledger tests compile the same source.
#[cfg(target_arch = "wasm32")]
pub use puzzle_term::bridge as protocol;
/// The web crate's core dependency is wasm-only; no native engine is needed here.
#[cfg(not(target_arch = "wasm32"))]
#[path = "../../puzzle-term/src/bridge/limits.rs"]
pub mod protocol;
