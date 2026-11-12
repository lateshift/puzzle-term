//! Ordered native raster page incarnations for frames larger than their atlas.
//!
//! Ordinary frames keep small dirty crops and a single render pass. Capacity
//! pressure owns the victim's complete pixels before resetting its shelf. The
//! executor draws references to that incarnation before copying another into
//! the same physical layer. These are upload staging bytes, not extra atlas
//! layers, and no address is moved or repacked. The final incarnation restores
//! the GPU mirror after all painter slots, including cursor and selection copies.
//! Retired keys are frame-local: a covered glyph repeated after a flush uses its
//! original ink, while the following frame rebuilds from the new cache generation.
//! Authored CPU bitmaps provide complete page bytes directly. GPU-created masks
//! provide ordered capture markers instead, because their pixels have no CPU mirror.
//! A mixed authored/grid page also uses a GPU capture: its sparse authored mirror
//! cannot reconstruct the generated masks before an in-frame reuse.
//! Empty page records track which GPU layers need a final capture after all crops.
//! Their earlier captures already sit before clears in the incremental dirty queue.
//! Only the final metadata marker leaves take_versions, avoiding a second capture
//! of overwritten ink under an old epoch. Retired keys remain usable by a late
//! cursor redraw in the same frame, with their immutable prior epoch.
use super::{BitmapKind, RasterAtlas};

/// One complete page image used at a capacity-triggered draw boundary.
pub struct RasterPageVersion {
    /// Mask and color pages have independent physical layer indices.
    pub kind: BitmapKind,
    /// Existing array layer reused after its earlier draws have completed.
    pub page: u32,
    /// Matches the stamp in each instance referencing these pixels.
    pub epoch: u16,
    /// Full mirror, including transparent gutters and unused shelf tails.
    pub pixels: Vec<u8>,
}

/// Transfers frame-local versions without making the text layer GPU-aware (R9).
impl RasterAtlas {
    /// Whether uploads must delimit native frame-local raster staging ownership.
    pub fn is_synchronous(&self) -> bool {
        self.synchronous
    }

    /// Owns a used victim before reuse; earlier handles remain meaningful this frame.
    pub(super) fn retain_version(&mut self, slot: usize) {
        let page = self.page_index(slot);
        let source = &self.pages[slot];
        if source.gpu {
            // The executor captures actual ink before the ordered clear; no readback occurs.
            self.dirty.push(super::RasterUpload {
                kind: source.kind,
                page,
                rect: super::AtlasRect {
                    x: 0,
                    y: 0,
                    width: super::PAGE,
                    height: super::PAGE,
                },
                pixels: Vec::new(),
                gpu: None,
                capture: Some(source.epoch),
            });
        }
        self.versions.push(RasterPageVersion {
            kind: source.kind,
            page,
            epoch: source.epoch,
            pixels: if source.gpu {
                Vec::new()
            } else {
                source.pixels.clone()
            },
        });
        // The shelf reset retires entries, but later draws may repeat this exact key.
        for (key, handle) in &self.entries {
            if handle.kind == source.kind && handle.page == page {
                self.retired.insert(*key, *handle);
            }
        }
    }

    /// Appends each reused layer's final image, then transfers all ordered versions.
    pub fn take_versions(&mut self) -> Vec<RasterPageVersion> {
        if self.versions.is_empty() {
            return Vec::new();
        }
        // Later unchanged frames must sample the current CPU mirror, not the last
        // older version drawn by a cursor or complementary selection scissor.
        for (slot, source) in self.pages.iter().enumerate() {
            let page = self.page_index(slot);
            if self
                .versions
                .iter()
                .any(|v| v.kind == source.kind && v.page == page)
            {
                self.versions.push(RasterPageVersion {
                    kind: source.kind,
                    page,
                    epoch: source.epoch,
                    pixels: if source.gpu {
                        Vec::new()
                    } else {
                        source.pixels.clone()
                    },
                });
            }
        }
        // Empty old records only marked reused GPU layers; their captures are already ordered.
        self.versions.retain(|v| {
            !v.pixels.is_empty()
                || self
                    .pages
                    .iter()
                    .filter(|p| p.kind == v.kind)
                    .nth(v.page as usize)
                    .is_some_and(|p| p.epoch == v.epoch)
        });
        std::mem::take(&mut self.versions)
    }
}
