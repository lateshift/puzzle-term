//! Harness access to the embedding-owned pty pool implementation.
//! The harness borrows bounded pool endpoints; allocation and handoff stay in the engine.
pub(crate) use puzzle_embed::diagnostics::pty_pool::*;
