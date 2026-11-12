//! Shared-endpoint contour packing with C1 row protection (design §9.2).
//!
//! Each curve's first texel holds p1 and p2; the following texel starts
//! with p3. The next connected curve overwrites that p3 texel's remaining
//! channels with its p2, so n curves cost n+1 texels unless C1 repeats the
//! endpoint at a row boundary. No shader curve fetch may start at column 4095.
//! The start address of each quadratic is stored in its band's list; the
//! second texel is implicit, so a row wrap there would read a wrong point.
use super::{AtlasError, SlugAtlas, TEXTURE_WIDTH};
use crate::shape::QuadShape;

/// Writes contours and returns texture coordinates for every curve.
impl SlugAtlas {
    /// Packs all connected segments before any band's index list is emitted.
    pub(super) fn pack_curves(&mut self, shape: &QuadShape) -> Result<Vec<[u16; 2]>, AtlasError> {
        let mut locations = vec![[0; 2]; shape.curves.len()];
        for contour in &shape.contours {
            for index in contour.clone() {
                let curve = shape.curves[index];
                // C1: the second texel is fetched as x+1 without wrapping.
                if self.curves.cursor % TEXTURE_WIDTH == TEXTURE_WIDTH - 1 {
                    self.curves.align_row();
                }
                let start = self.curves.cursor;
                self.curves
                    .ensure(start + 2, self.bands.bytes(), self.budget_bytes)?;
                let x = start % TEXTURE_WIDTH;
                let y = start / TEXTURE_WIDTH;
                locations[index] = [x as u16, y as u16];
                self.curves.texels[start] = [curve.p1[0], curve.p1[1], curve.p2[0], curve.p2[1]];
                self.curves.texels[start + 1][0] = curve.p3[0];
                self.curves.texels[start + 1][1] = curve.p3[1];
                self.curves.mark(start);
                self.curves.mark(start + 1);
                // A connected curve reuses p3 as p1 unless that texel is
                // column 4095. In that case p3 is repeated next row.
                let connected = index + 1 < contour.end && x + 1 < TEXTURE_WIDTH - 1;
                self.curves.cursor = start + if connected { 1 } else { 2 };
            }
        }
        Ok(locations)
    }
}
