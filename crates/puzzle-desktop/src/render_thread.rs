//! The process render actor is supplied by puzzle-embed (D-41/D-43).
//!
//! Window callbacks retain only public notification and surface endpoints.
//! The library owns the worker, display subscriptions and shutdown barrier.
pub(crate) use puzzle_embed::diagnostics::render_thread::*;
