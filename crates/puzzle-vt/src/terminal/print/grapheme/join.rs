//! Actual multi-scalar joins are separate from the scalar rejection path.
//! This cold code owns bounded UTF-8 construction, width changes and storage.
//! Scalar output retains the small inlined predecessor decision in grapheme.rs.
use super::{Cell, Property, State, Terminal, grapheme_width};

/// Rare joins share the same const-selected write observation as normal stores.
impl Terminal {
    /// Stores only a proven multi-scalar cluster; scalar rejection allocates nothing.
    // Separate rare joins so LLVM does not outline the predecessor decision
    // from the common print loop when both watch instantiations are linked.
    #[cold]
    #[inline(never)]
    pub(super) fn finish_grapheme<const WATCH: bool>(
        &mut self,
        next: char,
        property: Property,
        column: usize,
        cell: Cell,
        mut state: State,
    ) -> bool {
        let row_index = self.cursor.row;
        let Some(row) = self.screen().row(row_index) else {
            return false;
        };
        // A scalar predecessor has no String. Borrow any existing cluster before
        // acquiring its mutable storage, keeping rejected extensions allocation-free.
        let previous = row
            .extras
            .iter()
            .find(|extra| extra.column == column)
            .and_then(|extra| extra.cluster.as_deref());
        // Construct bytes only after the boundary decision accepted a real join.
        let mut bytes = [0_u8; 32];
        let Some(joined) = joined_text(previous, cell, next, &mut bytes) else {
            // Dropped marks do not enter either the stored text or its state.
            return true;
        };
        // Width is a property of the complete cluster, including selectors/ZWJ;
        // the scalar width used for its provisional first cell is insufficient.
        let width = grapheme_width(joined);
        self.note_cluster(joined.len());
        // DECAWM-off rejects late widening without storing either its text or
        // suffix. In particular, an older narrow cluster must remain intact.
        if self.late_widening_disabled(cell, column, width) {
            return true;
        }
        let Some(row) = self.screen_mut().row_mut(row_index) else {
            return false;
        };
        // Move a warm String from the row's extras/spares; never clone it merely
        // to append a mark. Storage is touched only after all rejection checks.
        let mut text = row.cluster_storage(column);
        text.push_str(joined);
        // Advance the suffix witness only after the bytes can be committed.
        // A byte-cap or disabled-widening refusal must retain the old suffix.
        state.push(property);
        self.store_grapheme::<WATCH>(row_index, column, cell, text, width);
        self.remember_grapheme(state);
        true
    }
}

/// Encodes one actual joined cluster into caller-owned bounded scratch.
/// None means the existing cluster has reached the §16 byte cap.
fn joined_text<'a>(
    previous: Option<&str>,
    cell: Cell,
    next: char,
    joined: &'a mut [u8; 32],
) -> Option<&'a str> {
    // Sparse predecessors use their authoritative text; ordinary cells use one
    // scalar. Replacement handles an internal sentinel without forming invalid UTF-8.
    let base = char::from_u32(cell.ch).unwrap_or('\u{fffd}');
    let length = previous.map_or_else(|| base.len_utf8(), str::len);
    // Refuse the whole new scalar rather than splitting UTF-8 at the security cap.
    if length + next.len_utf8() > joined.len() {
        return None;
    }
    // Existing bytes and the new scalar are copied into fixed stack scratch;
    // the returned slice borrows it only until row storage has been updated.
    if let Some(previous) = previous {
        joined[..length].copy_from_slice(previous.as_bytes());
    } else {
        base.encode_utf8(joined);
    }
    let end = length + next.encode_utf8(&mut joined[length..]).len();
    // Both inputs are valid UTF-8 and the splice is at a complete scalar boundary.
    std::str::from_utf8(&joined[..end]).ok()
}
