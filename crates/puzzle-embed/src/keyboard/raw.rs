//! Host callback facts are shared with recordings and session endpoints.
//!
//! The adapter borrows the same owned input types exposed by the embedding API;
//! this module keeps the recorded callback paths stable during the migration.
pub use crate::input::*;
