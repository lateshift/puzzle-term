//! Moves dirty Slug CPU mirror rows into a neutral frame (§9.8, §10.4).
//!
//! A prepared glyph's handle addresses physical texels in the shared
//! 4096-wide stores. Uploads keep those addresses, including row padding
//! inserted by packing rule B3, so the GPU samples the same shape as the
//! CPU oracle. No readback or per-cell conversion enters this path.
//! The CPU atlas can write a header after the band's curve list. Its
//! dirty-span tracker merges those writes into one physical row interval;
//! copying from the final mirror catches both in the order the GPU needs.
use puzzle_frame::{RenderFrame, Upload};
use puzzle_slug::{SlugAtlas, atlas::TEXTURE_WIDTH};

/// Drains each modified span after all current rows have prepared glyphs.
pub(super) fn append(frame: &mut RenderFrame, atlas: &mut SlugAtlas) {
    let (curves, bands) = atlas.take_dirty();
    for span in curves {
        // A span is local to one 4096-texel row, so its end is exclusive
        // inside that row and cannot upload a neighboring glyph's texels.
        let first = usize::from(span.row) * TEXTURE_WIDTH + usize::from(span.start);
        let last = usize::from(span.row) * TEXTURE_WIDTH + usize::from(span.end);
        frame.uploads.push(Upload::Curves {
            origin: [u32::from(span.start), u32::from(span.row)],
            texels: atlas.curve_texels()[first..last].to_vec(),
        });
    }
    for span in bands {
        // Band index planes use two u16 values per texel. The same byte
        // address as the CPU handle is preserved by the physical origin.
        let first = usize::from(span.row) * TEXTURE_WIDTH + usize::from(span.start);
        let last = usize::from(span.row) * TEXTURE_WIDTH + usize::from(span.end);
        frame.uploads.push(Upload::Bands {
            origin: [u32::from(span.start), u32::from(span.row)],
            texels: atlas.band_texels()[first..last].to_vec(),
        });
    }
}
