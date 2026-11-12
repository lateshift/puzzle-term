//! Most native events own JSON bytes. Precharged reply events instead borrow
//! the handler's immutable allocation, including its already written receipt
//! prefix, so a full extension state table can still return an error without an
//! unaccounted second payload allocation during C callback translation.
use puzzle_term::platform::reply::ReplyLease;

/// Backing ownership lasts through the entire foreign callback invocation.
pub(crate) enum Payload {
    /// Existing facts, input bytes and ordinary VT replies retain Vec ownership.
    Owned(Vec<u8>),
    /// Native optional effects share exactly one precharged immutable buffer.
    /// A foreign callback borrows its used slice without making a JSON copy.
    Osc(std::sync::Arc<Vec<u8>>),
    /// Optional protocol storage is shared, immutable and precharged to its cap.
    Precharged(ReplyLease),
}
impl From<Vec<u8>> for Payload {
    /// Existing event encoders transfer their allocation without another copy.
    fn from(bytes: Vec<u8>) -> Self {
        Self::Owned(bytes)
    }
}
impl std::ops::Deref for Payload {
    /// memory::borrowed sees only the used callback slice, never stale capacity.
    type Target = [u8];
    /// A precharged frame already includes its little-endian receipt ID.
    fn deref(&self) -> &[u8] {
        match self {
            Self::Owned(bytes) => bytes,
            Self::Osc(bytes) => bytes.as_slice(),
            Self::Precharged(lease) => lease.framed(),
        }
    }
}
