//! Pixel equality for two terminal-cell spans in a rendered frame.
//!
//! The comparison removes only each row's device-pixel offset. It keeps
//! RGB and alpha unchanged, so a VS16 tofu mark or a shifted following
//! glyph changes an actual readback byte and fails the assertion.

use super::CellGeometry;
use puzzle_render::RgbaImage;

/// One exclusive-endpoint span in a single terminal row.
struct Span {
    /// Zero-based terminal row.
    row: u32,
    /// First included terminal column.
    start: u32,
    /// First excluded terminal column.
    end: u32,
}

/// Compares two cell spans of equal physical size, including their blanks.
pub(crate) fn check(
    image: &RgbaImage,
    grid: CellGeometry,
    first: &str,
    second: &str,
) -> Result<(), String> {
    // Parsing both spans against the same terminal grid rejects a
    // mislabeled row before any bytes are compared.
    let left = parse(first, &grid)?;
    let right = parse(second, &grid)?;
    // Comparing unequal widths would hide shifted following text or
    // truncate the wide side at precisely the error under test.
    if left.end - left.start != right.end - right.start {
        return Err("cell spans have different widths".into());
    }
    let width = (left.end - left.start) * grid.width;
    let x_left = 16 + left.start * grid.width;
    let x_right = 16 + right.start * grid.width;
    let y_left = 16 + left.row * grid.height;
    let y_right = 16 + right.row * grid.height;
    // Check the exact cell rectangles before indexing. A caller should not
    // be able to compare a clipped sliver and call the cells identical.
    if x_left + width > image.width
        || x_right + width > image.width
        || y_left + grid.height > image.height
        || y_right + grid.height > image.height
    {
        return Err("cell span exceeds readback image".into());
    }
    // Iterate physical scanlines, because hinted outlines and Slug
    // antialiasing can differ in a single edge pixel.
    for dy in 0..grid.height {
        // One packed RGBA row can be compared directly. The x coordinate
        // changes, but each sample covers the same number of device pixels.
        let first_offset = (((y_left + dy) * image.width + x_left) * 4) as usize;
        let second_offset = (((y_right + dy) * image.width + x_right) * 4) as usize;
        let byte_count = (width * 4) as usize;
        // This is literal byte equality, including alpha. A tolerance
        // would permit a pale selector artifact to slip through.
        if image.pixels[first_offset..first_offset + byte_count]
            != image.pixels[second_offset..second_offset + byte_count]
        {
            return Err(format!(
                "cell pixels differ: {first} vs {second}, device row {dy}"
            ));
        }
    }
    Ok(())
}

/// Parses `row:first-last` without accepting an out-of-grid endpoint.
fn parse(value: &str, grid: &CellGeometry) -> Result<Span, String> {
    // The scenario grammar uses one row and two exclusive column bounds;
    // it cannot accidentally compare diagonal rectangles.
    let (row, columns) = value.split_once(':').ok_or("cell span needs row:columns")?;
    let (start, end) = columns
        .split_once('-')
        .ok_or("cell span needs first-last")?;
    let span = Span {
        row: row.parse().map_err(|_| "invalid cell row")?,
        start: start.parse().map_err(|_| "invalid first column")?,
        end: end.parse().map_err(|_| "invalid last column")?,
    };
    // An empty span could pass vacuously; a span reaching past the grid
    // could compare only padding and miss the glyph under test.
    if span.row >= grid.rows || span.start >= span.end || span.end > grid.cols {
        return Err("cell span is outside terminal grid".into());
    }
    Ok(span)
}
