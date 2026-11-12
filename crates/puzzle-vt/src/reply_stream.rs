//! Lazy protocol encoding for replies larger than a transport's pending allowance.
//! The pure VT owns no writer or completion queue. A Session takes this source
//! at its ordered parser boundary and encodes a packet only when credit permits.
//! Binary data remains in its original protocol-bounded immutable allocation.

/// A bounded packet generator; implementations never perform I/O or wait.
pub trait ReplySource: Send {
    /// Appends one complete wire packet, or returns false after its final packet.
    /// Each packet must fit the existing OSC buffer allowance. The caller clears
    /// and reuses out between calls; false must not append any bytes.
    fn append_next(&mut self, out: &mut Vec<u8>) -> bool;
}
