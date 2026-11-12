//! Geometry and actual retained ownership for one cold reflow operation.
//! The store keeps its original String allocation throughout relocation. A
//! String clone may have capacity equal to length, so it is neither needed for
//! placement nor a valid source for the live record's storage charge.
use crate::multicell::{Fragment, MulticellStore};
use smallvec::SmallVec;

/// Short-lived relocation data contains no cloned payload or independent byte formula.
pub(super) struct SourceBlock {
    /// Source column addresses rendition and tracked-position witnesses.
    pub(super) column: usize,
    /// Whole rectangle width must fit the destination before any cell is copied.
    pub(super) columns: usize,
    /// Original occupied height bounds resize admission, including clipped roots.
    pub(super) scale: u8,
    /// At most seven inline coordinates, preserving original vertical offsets.
    pub(super) fragments: SmallVec<[Fragment; 7]>,
    /// Live record's exact bill from the store's shared accounting definition.
    pub(super) charge: usize,
}

/// Relocation changes coordinates only; payload ownership remains with the store.
impl SourceBlock {
    /// Captures geometry and charge from the same live record borrow.
    pub(super) fn capture(store: &MulticellStore, key: u32) -> Option<Self> {
        let block = store.get(key)?;
        // Reflow's fit/copy stages cannot append, replace or free this payload.
        // The captured bill therefore remains valid until all fragments move;
        // record_bytes is also used for admission, erase and owner transfer.
        Some(Self {
            column: block.column,
            columns: block.columns,
            scale: block.sizing.scale,
            fragments: block.fragments.clone(),
            charge: MulticellStore::record_bytes(block),
        })
    }
}
