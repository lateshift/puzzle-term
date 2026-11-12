//! Caller-owned font bytes at the platform-to-text boundary (design §8.1).
//!
//! The browser fetches these bytes and the native harness reads them from
//! `assets/fonts/`. A shared immutable allocation keeps the parsed face's
//! source alive without embedding a font in a platform-neutral crate or
//! duplicating its bytes for every Slug scene. The face index is retained
//! for future collections; the M3 probe only accepts index zero.
use std::sync::Arc;

/// Immutable bytes and collection face index supplied by a platform shell.
#[derive(Clone)]
pub struct FontData {
    /// OpenType bytes shared among scene preparations.
    bytes: Arc<[u8]>,
    /// Face number in a collection, zero for the bundled TTF.
    index: u32,
}

impl FontData {
    /// Takes ownership of fetched or file-read bytes without another copy.
    pub fn new(bytes: Vec<u8>, index: u32) -> Self {
        Self {
            bytes: bytes.into(),
            index,
        }
    }

    /// Borrows the immutable OpenType input for skrifa's lazy table reads.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Reports the collection face requested by the platform.
    pub fn index(&self) -> u32 {
        self.index
    }
}
