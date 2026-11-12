//! Byte ownership for browser writer handoffs, without copying precharged replies.
//! A lease can be shared between a socket and callback without cloning its payload.
//! into_vec is a compatibility fallback; reply paths preserve the leased variant.
//! Used bytes exclude the native callback prefix and unused allocation tail.

/// Exact transport ownership avoids copying precharged optional-protocol replies.
pub(in crate::runtime) enum WriteData {
    /// Existing user input and ordinary VT batches retain Vec ownership.
    Owned(Vec<u8>),
    /// Runtime and transport share one metered immutable allocation.
    Leased(puzzle_term::platform::reply::ReplyLease),
}
impl WriteData {
    /// Browser APIs see used wire bytes only.
    pub(in crate::runtime) fn as_slice(&self) -> &[u8] {
        match self {
            Self::Owned(bytes) => bytes,
            Self::Leased(lease) => lease.payload(),
        }
    }
    /// Keep a lease alive until another host route acknowledges consumption.
    pub(in crate::runtime) fn lease(&self) -> Option<puzzle_term::platform::reply::ReplyLease> {
        match self {
            Self::Owned(_) => None,
            Self::Leased(lease) => Some(lease.clone()),
        }
    }
    /// Only ordinary input takes this fallback; reply routes retain their lease.
    pub(in crate::runtime) fn into_vec(self) -> Vec<u8> {
        match self {
            Self::Owned(bytes) => bytes,
            Self::Leased(lease) => lease.payload().to_vec(),
        }
    }
}

/// Reply ownership survives effect translation without a second payload allocation.
/// Socket and host routes may retain immutable leases through actual consumption.
impl From<puzzle_term::platform::reply::ReplyBytes> for WriteData {
    /// A Vec-backed VT batch reuses its allocation; a prepaid buffer stays shared.
    fn from(bytes: puzzle_term::platform::reply::ReplyBytes) -> Self {
        match bytes {
            puzzle_term::platform::reply::ReplyBytes::Owned(bytes) => Self::Owned(bytes.into_vec()),
            puzzle_term::platform::reply::ReplyBytes::Precharged(lease) => Self::Leased(lease),
        }
    }
}
