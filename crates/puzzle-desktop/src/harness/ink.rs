//! Measures GPU ink in either terminal cells or explicit device pixels.
//!
//! `expect-ink` uses row:column endpoints and the font metrics that built
//! the current terminal frame. A separate `expect-ink-px` keeps Slug and
//! isolated text specimens independent of a terminal grid. Mixing the two
//! used to let a broad, misplaced pixel rectangle pass a cell assertion.
//!
//! The clear pixel supplies the background because terminal themes can
//! change it. Opaque offscreen targets make alpha unsuitable for occupancy.
use puzzle_render::RgbaImage;

/// Compare two cell spans byte-for-byte within a single rendered frame.
pub(super) mod identical;

/// Terminal dimensions and device-pixel geometry of the rendered grid.
pub(super) struct CellGeometry {
    /// Number of visible terminal columns.
    pub(super) cols: u32,
    /// Number of visible terminal rows.
    pub(super) rows: u32,
    /// Primary font's physical cell width.
    pub(super) width: u32,
    /// Primary font's physical cell height.
    pub(super) height: u32,
}

/// Checks occupied RGB pixels within one exclusive-endpoint rectangle.
pub(super) fn check<'a>(
    image: &RgbaImage,
    parts: &mut impl Iterator<Item = &'a str>,
    cells: Option<CellGeometry>,
) -> Result<(), String> {
    // Bounds are supplied by a reviewed scenario. Reject incomplete
    // commands before reading a possibly stale screenshot.
    let bounds = parts.next().ok_or("ink assertion needs a range")?;
    let min = percent(parts.next().ok_or("ink assertion needs minimum")?)?;
    let max = percent(parts.next().ok_or("ink assertion needs maximum")?)?;
    // Reversed thresholds are almost always a typo. They must not yield
    // a passing empty interval by accident.
    if min > max {
        return Err("ink minimum exceeds maximum".into());
    }
    let (x0, y0, x1, y1) = rectangle(bounds, image, cells)?;
    // Every terminal frame leaves a 16-pixel inset at the upper left,
    // making this pixel the theme's current default background.
    let background = image.pixels.get(..3).ok_or("empty readback image")?;
    let mut ink = 0usize;
    // Readback rows are tightly packed RGBA8, with no GPU row padding.
    for y in y0..y1 {
        for x in x0..x1 {
            let offset = ((y * image.width + x) * 4) as usize;
            if image.pixels[offset..offset + 3] != *background {
                ink += 1;
            }
        }
    }
    let total = (x1 - x0) as usize * (y1 - y0) as usize;
    // Area normalization makes the same lower bound meaningful for a
    // one-cell glyph and a larger decorated row region.
    let share = ink as f64 * 100.0 / total as f64;
    // Both bounds matter: blank output and an accidentally solid quad
    // should each fail before a golden comparison is consulted.
    if share < min || share > max {
        return Err(format!("ink {share:.2}% outside {min:.2}%..{max:.2}%"));
    }
    Ok(())
}

/// Converts cell or pixel endpoints into a checked image rectangle.
fn rectangle(
    bounds: &str,
    image: &RgbaImage,
    cells: Option<CellGeometry>,
) -> Result<(u32, u32, u32, u32), String> {
    // Endpoints are exclusive so neighbouring cell ranges never share
    // pixels at their common edge.
    let (start, end) = bounds.split_once('-').ok_or("invalid ink range")?;
    let (first, second) = (coordinate(start)?, coordinate(end)?);
    let (x0, y0, x1, y1) = if let Some(grid) = cells {
        // A terminal frame has a fixed 16-device-pixel inset (§10.5).
        // Check the declared cell range before mapping it to pixels; a
        // clipped out-of-grid request must never silently pass.
        if first.0 >= second.0
            || first.1 >= second.1
            || second.0 > grid.rows
            || second.1 > grid.cols
        {
            return Err("ink cell range is outside terminal grid".into());
        }
        (
            16 + first.1 * grid.width,
            16 + first.0 * grid.height,
            16 + second.1 * grid.width,
            16 + second.0 * grid.height,
        )
    } else {
        // Pixel ranges are useful for freeform Slug and text scenes, whose
        // image dimensions can differ from a terminal cell grid.
        (first.0, first.1, second.0, second.1)
    };
    // Pixel mode also rejects out-of-image ranges. Earlier clipping hid
    // a mistaken cell coordinate by shrinking it into a valid crop.
    if x0 >= x1 || y0 >= y1 || x1 > image.width || y1 > image.height {
        return Err("ink range is empty or outside readback image".into());
    }
    Ok((x0, y0, x1, y1))
}

/// Parses a row:column pair for cells or an x:y pair for pixels.
fn coordinate(value: &str) -> Result<(u32, u32), String> {
    let (first, second) = value.split_once(':').ok_or("ink coordinate needs ':'")?;
    Ok((
        first.parse().map_err(|_| "invalid first ink coordinate")?,
        second
            .parse()
            .map_err(|_| "invalid second ink coordinate")?,
    ))
}

/// Rejects NaN, infinity and values outside a percentage's range.
fn percent(value: &str) -> Result<f64, String> {
    let number = value.strip_suffix('%').ok_or("ink bound needs %")?;
    let value: f64 = number.parse().map_err(|_| "invalid ink percentage")?;
    if !value.is_finite() || !(0.0..=100.0).contains(&value) {
        return Err("ink percentage outside 0..100".into());
    }
    Ok(value)
}

#[cfg(test)]
/// Pixel and cell assertions are exercised without a GPU dependency.
mod tests;
