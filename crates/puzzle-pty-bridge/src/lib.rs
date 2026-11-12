//! Native development bridge, deliberately independent of the terminal engine.
//!
//! Only the wire codec is shared with puzzle-term: compiling that same pure
//! source module avoids a Cargo dependency on fonts, the VT or the renderer.
//! HTTP authorization precedes the first WebSocket message; PTY creation
//! follows a validated v1 hello. Each accepted session has bounded queues.
/// Native exit readiness and retained disconnect cleanup.
mod child;
/// Command-line choices and the explicitly authorized page origins.
pub mod config;
/// One authenticated socket's protocol state machine and cleanup.
mod connection;
/// Bounded output credit shared by the socket and blocking PTY reader.
mod flow;
/// Native event-loop counters for idle and input-pressure evidence.
pub mod metrics;
/// Shared little-endian protocol, with no platform APIs or terminal types.
#[path = "../../puzzle-term/src/bridge.rs"]
pub mod protocol;
/// PTY spawning, ordered input and output draining until child exit.
mod pty;
/// Delay/length/bytes recording of the output stream only.
mod record;
/// Token creation and handshake authorization before a shell can start.
mod security;
/// Loopback listener and four-session admission control.
pub mod server;
/// Host-driven listener stop without polling.
pub mod shutdown;
/// Async HTTP authorization and initial protocol negotiation.
mod wire;
/// Bounded async output sink with FIFO exit ordering.
mod writer;

/// Credited stdin storage and actual-write readiness, kept native to the bridge.
mod input;
/// Retained response ownership keeps socket controls readable under congestion.
mod response;
