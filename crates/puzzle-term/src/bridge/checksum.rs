//! Streaming FNV-1a for native and browser bridge transfer diagnostics.
//!
//! This is a position-sensitive loss/reordering witness, not authentication.
//! Keeping the basis and update together prevents the two targets from using
//! different byte-stream definitions. No buffering or platform APIs are needed.

/// Standard 64-bit FNV-1a offset basis, used before the first payload byte.
pub const FNV1A_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// Extends a prior checksum in wire order and returns the state for the next chunk.
/// An iterator also lets fixtures repeat a corpus without allocating 100 MB.
pub fn fnv1a_update(mut checksum: u64, bytes: impl IntoIterator<Item = u8>) -> u64 {
    // Wrapping multiplication is the specified modulo-2^64 FNV operation.
    // Continuing with the previous state makes packet boundaries irrelevant.
    for byte in bytes {
        checksum = (checksum ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
    }
    checksum
}
