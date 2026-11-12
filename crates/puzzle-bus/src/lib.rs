//! Generic, platform-neutral command and event bus (design §5).
//!
//! Domain crates own their concrete command and event enums. This leaf crate
//! queues any pair implementing `Message`, preserving policy and observation
//! semantics without importing terminal, GPU or platform types (R9).

/// Deterministic queued dispatch and cascade control.
pub mod bus;
/// Stable diagnostic event names for warnings and errors.
pub mod diagnostics;
/// Interceptor, observer and byte-tap contracts.
pub mod hooks;
/// Generic catalogue metadata and public-name lookup.
pub mod kinds;
/// Tracing filter parsing and the in-memory test layer.
pub mod logging;
/// Message contract and trusted command origins.
pub mod message;
/// Neutral plain-data tree shared with future settings.
pub mod value;
/// Validated conversion at the external bus boundary.
pub mod wire;

pub use bus::Bus;
pub use message::{Message, Origin};
pub use value::Value;
