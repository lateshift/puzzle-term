//! Kitty graphics placement and source-rectangle interpretation.
//! Reference: <https://sw.kovidgoyal.net/kitty/graphics-protocol/#display-images-on-screen>.
//! Cell-anchored geometry and source lifetime rules share VT's tracked registry.
//! A full erase retires placements; history erasure retires only evicted anchors.
use crate::images::{ImageError, Placement, Store, control::Control};
use crate::vt::Terminal;

/// Validated device-pixel crop and cell-relative placement are committed together.
pub(crate) struct Geometry {
    /// Source intersection in decoded image pixels.
    source: [u32; 4],
    /// Device-pixel displacement within the tracked anchor cell.
    offset: [u32; 2],
    /// Display rectangle in grid cells, including any natural-aspect padding.
    cells: [u32; 2],
}

/// Derives the unspecified cell dimension with checked source aspect arithmetic.
fn aspect(n: u32, from: u32, to: u32, from_cell: u32, to_cell: u32) -> Result<u32, ImageError> {
    // Convert cells to device pixels before applying the source image ratio.
    let pixels = u64::from(n) * u64::from(from_cell);
    let scaled = pixels
        .checked_mul(u64::from(to))
        .ok_or(ImageError::Invalid)?;
    // Round up so the placement covers every source pixel at the derived scale.
    u32::try_from(scaled.div_ceil(u64::from(from) * u64::from(to_cell)))
        .map_err(|_| ImageError::Invalid)
}

/// Geometry is validated before decoded image replacement can retire older content.
impl Store {
    /// Resolves crop/size/offset without allocating or creating a tracked handle.
    pub(crate) fn validate_put(
        &self,
        size: [u32; 2],
        control: &Control,
        terminal: &Terminal,
    ) -> Result<Geometry, ImageError> {
        // Source coordinates are decoded-image pixels, not terminal cells.
        // Saturating intersection rejects an origin outside the source without
        // allowing unsigned subtraction to turn it into a huge rectangle.
        let x = control.number(b'x', 0)?;
        let y = control.number(b'y', 0)?;
        // Width/height defaults select the complete source. Intersecting with
        // the remaining extent clips a crop extending past its right/bottom edge.
        let w = control
            .number(b'w', size[0])?
            .min(size[0].saturating_sub(x));
        let h = control
            .number(b'h', size[1])?
            .min(size[1].saturating_sub(y));
        // An empty intersection has no drawable image. Reject it before any
        // aspect-ratio division or tracked-position allocation can happen.
        if w == 0 || h == 0 {
            return Err(ImageError::Invalid);
        }
        // The renderer sends device-pixel cell geometry to the Session. Before
        // the first metric message, the nonzero fallback prevents division by zero.
        let cell = [
            u32::from(terminal.cell_pixels.0).max(1),
            u32::from(terminal.cell_pixels.1).max(1),
        ];
        // X/Y are physical offsets inside the first cell, unlike x/y above.
        // They must not cross its edge, even when c/r request a larger rectangle.
        let offset = [control.number(b'X', 0)?, control.number(b'Y', 0)?];
        if offset[0] >= cell[0] || offset[1] >= cell[1] {
            return Err(ImageError::Invalid);
        }
        let cells = cell_extent(control, [w, h], offset, cell)?;
        // Metadata has its own bound derived from the same allowance, not a new setting.
        // Byte-derived metadata admission also bounds repeated anonymous puts.
        // Replacing the same nonzero ID releases its old entry before counting.
        if self.retained_placements(control)? >= self.limit / std::mem::size_of::<Placement>() {
            return Err(ImageError::NoSpace);
        }
        Ok(Geometry {
            source: [x, y, w, h],
            offset,
            cells,
        })
    }

    /// Replacement admission counts the table after removing the replaced instance.
    /// A full metadata table must still allow a program to move its existing image.
    fn retained_placements(&self, control: &Control) -> Result<usize, ImageError> {
        // A new source cannot replace any old placement metadata. Existing put
        // requests may reuse a nonzero placement ID without needing a free slot.
        let Ok(index) = self.find(control) else {
            return Ok(self.placements.len());
        };
        // IDs can be reused across transmissions, so use the retained source's
        // private generation when deciding which old placements will disappear.
        let generation = self.images[index].generation;
        let id = control.number(b'p', 0)?;
        let replacing_source = control.letter(b'a', b't')? == b'T';
        Ok(self
            .placements
            .iter()
            .filter(|placement| {
                placement.image != generation
                    || !replacing_source && (id == 0 || placement.id != id)
            })
            .count())
    }

    /// Places retained pixels at the final-chunk cursor; nonzero IDs replace atomically.
    pub(crate) fn put(
        &mut self,
        index: usize,
        control: &Control,
        terminal: &mut Terminal,
    ) -> Result<(), ImageError> {
        // Read all geometry before mutating either source recency or handles.
        // This keeps failed puts invisible to the application's previous image.
        let image = &self.images[index];
        let Geometry {
            source,
            offset,
            cells,
        } = self.validate_put(image.pixels.size, control, terminal)?;
        // Use the final-chunk cursor, after any interleaved ordinary text.
        // Tracked positions resolve row identity across future scroll and reflow.
        let line = terminal
            .screen()
            .row(terminal.cursor.row)
            .ok_or(ImageError::Invalid)?
            .id;
        let generation = image.generation;
        // p has no meaning without a nonzero image ID. Keeping zero here allows
        // multiple anonymous placements exactly as the wire protocol specifies.
        let id = if image.id == 0 {
            0
        } else {
            control.number(b'p', 0)?
        };
        let z = control.z()?;
        self.replace_placement(generation, id, terminal);
        // The registry tags this handle with the active screen's independent
        // LineId domain; alternate row IDs cannot collide with retained history.
        // Virtual prototypes use this bounded handle only as an instance identity;
        // its position/liveness never controls their visibility or lifetime.
        let anchor = terminal.tracked.track(line, terminal.cursor.column);
        // Store source crop and cell extent separately: resizing changes grid
        // geometry, while the same decoded pixel rectangle remains the source.
        self.placements.push(Placement {
            image: generation,
            virtual_: control.number(b'U', 0)? == 1,
            id,
            anchor,
            screen: terminal.active,
            source,
            offset,
            cells,
            requested_cells: [control.number(b'c', 0)?, control.number(b'r', 0)?],
            z,
        });
        // Successful display makes a source recent for decoded LRU admission.
        // Failed placement attempts deliberately never update this logical clock.
        self.serial = self.serial.wrapping_add(1);
        self.images[index].used = self.serial;
        advance_cursor(control, cells, terminal)?;
        terminal.screen_mut().damage.full = true;
        Ok(())
    }
    /// Releases a previous explicit instance before tracking its replacement.
    /// Matching uses source generation as wire image IDs can be reused.
    /// Anonymous instances coexist and therefore never retire one another.
    fn replace_placement(&mut self, generation: u64, id: u32, terminal: &mut Terminal) {
        // Anonymous placements are intentionally never replaced by p=0.
        // Nonzero pairs identify one instance, even across primary/alternate views.
        if id != 0 {
            self.placements.retain(|placement| {
                if placement.image != generation || placement.id != id {
                    return true;
                }
                terminal.tracked.release(placement.anchor);
                false
            });
        }
    }
}

/// Resolves explicit or natural cell extents after the crop and offsets validate.
/// Source and offsets are device pixels; the returned dimensions are grid cells.
/// This calculation borrows metadata only and cannot change anchors or storage.
/// Checked arithmetic rejects an unrepresentable footprint before committing it.
fn cell_extent(
    control: &Control,
    source: [u32; 2],
    offset: [u32; 2],
    cell: [u32; 2],
) -> Result<[u32; 2], ImageError> {
    let [w, h] = source;
    let mut cells = [control.number(b'c', 0)?, control.number(b'r', 0)?];
    // Natural-size images cover enough cells for their pixels plus offsets.
    // A single explicit dimension derives the other from the source aspect;
    // both explicit dimensions are retained for letterboxing at render time.
    match cells {
        [0, 0] => {
            cells = [
                w.checked_add(offset[0])
                    .ok_or(ImageError::Invalid)?
                    .div_ceil(cell[0]),
                h.checked_add(offset[1])
                    .ok_or(ImageError::Invalid)?
                    .div_ceil(cell[1]),
            ]
        }
        [0, r] => cells[0] = aspect(r, h, w, cell[1], cell[0])?,
        [c, 0] => cells[1] = aspect(c, w, h, cell[0], cell[1])?,
        _ => {}
    }
    Ok(cells)
}

/// Applies the protocol cursor side effect only after placement commitment.
fn advance_cursor(
    control: &Control,
    cells: [u32; 2],
    terminal: &mut Terminal,
) -> Result<(), ImageError> {
    if control.number(b'C', 0)? == 0 && control.number(b'U', 0)? == 0 {
        // Kitty's "Controlling displayed image layout" moves down by r, not r−1:
        // <https://sw.kovidgoyal.net/kitty/graphics-protocol/#controlling-displayed-image-layout>.
        // Line feeds respect margins and scroll; at most one screen plus a row
        // is visited, since off-screen cursor positioning is protocol-undefined.
        let (columns, rows) = terminal.size();
        for _ in 0..(cells[1] as usize).min(rows + 1) {
            terminal.advance(b"\n");
        }
        // Clamp horizontal motion at the margin instead of iterating over
        // untrusted c cells. pending_wrap must not leak from earlier text.
        terminal.cursor.column = terminal
            .cursor
            .column
            .saturating_add(cells[0] as usize)
            .min(columns - 1);
        terminal.cursor.pending_wrap = false;
    }
    Ok(())
}
