//! Harness access to the embedding-owned session thread implementation.
//! Only messages and sealed slots cross this re-export; mutable Session stays on its worker.
pub(crate) use puzzle_embed::diagnostics::session_thread::*;
