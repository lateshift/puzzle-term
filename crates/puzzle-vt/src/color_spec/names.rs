//! Sorted X11 names, generated from the approved MIT/X11 rgb.txt data.
//! Source, licence and regeneration instructions: assets/colors/README.md.
//! Records are packed to avoid a pointer/length pair per name in native builds.
//! Binary search folds the query as an iterator: no allocation or startup work.
use std::cmp::Ordering;

/// Immutable little-endian offsets followed by RGB/name records.
const TABLE: &[u8] = include_bytes!("../../../../assets/colors/x11-colours.bin");

/// Reads a generated u16; all offsets are checked by the generator and tests.
fn number(offset: usize) -> usize {
    usize::from(u16::from_le_bytes([TABLE[offset], TABLE[offset + 1]]))
}

/// Finds a name with XParseColor's ASCII case and space normalization.
pub(super) fn lookup(name: &[u8]) -> Option<[u8; 3]> {
    let mut low = 0;
    let mut high = number(0);
    // Search compares normalized bytes directly, never constructs a String.
    while low < high {
        let middle = low + (high - low) / 2;
        // The sentinel offset supplies the final record's exclusive end.
        let start = number(2 + middle * 2);
        let end = number(4 + middle * 2);
        // RGB occupies three bytes; only the trailing name participates in ordering.
        let key = &TABLE[start + 3..end];
        // Skip spaces in the query on each comparison; no temporary key is built.
        let order = key.iter().copied().cmp(
            name.iter()
                .copied()
                .filter(|b| *b != b' ')
                .map(|b| b.to_ascii_lowercase()),
        );
        match order {
            Ordering::Less => low = middle + 1,
            Ordering::Greater => high = middle,
            Ordering::Equal => return Some([TABLE[start], TABLE[start + 1], TABLE[start + 2]]),
        }
    }
    None
}
