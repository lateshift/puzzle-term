//! Sparse extras follow the same bounded column splice as their owning cells.
//! Boundary preparation has already removed every split atomic glyph before
//! this metadata moves; unrelated prefix/suffix extras retain their positions.
/// Keeps sparse column metadata aligned with shifted cells.
pub(super) fn shift_extras(
    row: &mut crate::Row,
    left: usize,
    right: usize,
    count: usize,
    insert: bool,
) {
    if insert {
        // Inserted columns push entries right; entries beyond the margin
        // fall out with the cells they used to decorate.
        row.extras.retain(|extra| {
            extra.column < left || extra.column > right || extra.column + count <= right
        });
        // Filtering precedes translation: an extra pushed outside the right
        // margin must not survive beside an unrelated unchanged outside cell.
        for extra in &mut row.extras {
            if (left..=right).contains(&extra.column) {
                extra.column += count;
            }
        }
    } else {
        // Deleted columns lose their metadata; surviving entries move
        // left while retaining sort order for binary lookup.
        row.extras.retain(|extra| {
            extra.column < left || extra.column > right || extra.column >= left + count
        });
        // All surviving suffix columns move by the same count, preserving
        // the sorted order required by subsequent binary-search lookups.
        for extra in &mut row.extras {
            if (left + count..=right).contains(&extra.column) {
                extra.column -= count;
            }
        }
    }
}
