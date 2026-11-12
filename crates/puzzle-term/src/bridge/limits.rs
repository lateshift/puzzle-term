//! Byte budgets for both sides of the development bridge (design §12.3).
//!
//! Input and output have independent ledgers even though their windows are
//! the same size. Returning parsed output credit must never grant stdin space.
//! The payload chunk bounds PTY reservations and the codec's message size;
//! the ACK threshold controls when consumed output returns read credit.

/// Initial and maximum outstanding input credit, replenished after OS writes.
pub const INPUT_WINDOW: u32 = 1024 * 1024;
/// PTY output reads pause above this unacknowledged byte count (§12.3).
pub const OUTPUT_WINDOW: usize = 1024 * 1024;
/// Parsed output accumulates this many bytes before the client sends an ACK.
pub const ACK_THRESHOLD: usize = 64 * 1024;
/// Largest input payload and PTY read; bounds the single output overshoot.
pub const PAYLOAD_CHUNK: usize = 64 * 1024;
