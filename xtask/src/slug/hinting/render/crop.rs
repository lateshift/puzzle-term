//! Nearest-neighbor evidence enlargement for D-27 review.
//!
//! The main sheet is at native DPR 1. Image viewers may interpolate when
//! zooming, so each variant also gets an exact 3× byte replication. That
//! makes a single gray edge pixel visible as a 3×3 square without adding
//! any new rasterization or a third-party image tool.
use super::ROW_HEIGHT;
use puzzle_slug::reference::OracleImage;

/// Copies one variant's five rows and magnifies each byte by nearest neighbor.
pub(crate) fn crop_threefold(image: &OracleImage, first_row: u32, row_count: u32) -> OracleImage {
    let top = first_row * ROW_HEIGHT;
    let height = row_count * ROW_HEIGHT;
    let width3 = image.width * 3;
    let height3 = height * 3;
    let mut pixels = vec![0; width3 as usize * height3 as usize * 4];
    for y in 0..height3 {
        for x in 0..width3 {
            // Integer division duplicates each source pixel as a 3×3 block;
            // no interpolation can hide one-pixel gaps in the review crops.
            let source = (((top + y / 3) * image.width + x / 3) * 4) as usize;
            let target = ((y * width3 + x) * 4) as usize;
            pixels[target..target + 4].copy_from_slice(&image.pixels[source..source + 4]);
        }
    }
    OracleImage {
        width: width3,
        height: height3,
        pixels,
    }
}
