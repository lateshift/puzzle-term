//! Precharged immutable reply storage can outlive a full optional-protocol table.
//! The protocol keeps one owner reference and accounts this allocation up front.
//! A writer/host borrows a lease, never clones the bytes.
//! Reuse requires every immutable lease to be gone.
//!
//! The extra eight bytes belong to the C completion envelope, not PTY output.
//! Keeping them in the same allocation avoids a second payload at full state.
//! Arc's counters also occupy retained memory, so allocated_bytes includes them.
//! The runtime separately charges its inline ReusableReply and pending-op fields.
//! Reservation is transactional: uniqueness and length are checked before credit.
//! Once leased, the allocation is immutable until every writer/callback releases it.
//! Writer acknowledgement alone cannot grant mutable access to a borrowed callback.
//! Lease drops therefore post readiness after releasing their storage reference.
//! Readiness conveys no credit; only the validated writer ticket returns bytes.
//! A receiver may wake early and still see another lease; a later drop wakes again.
//! No count-based last-drop guess is needed, avoiding concurrent-release races.
//! Teardown can discard the owner while immutable transport leases remain valid.
//! Completion must drop writer ownership before notifying the producer.
use super::ReplyTicket;
use std::sync::Arc;

/// A reusable packet allocation, including the C callback's u64 receipt prefix.
/// Charge allocated_bytes once while the protocol or any writer lease retains it.
pub struct ReusableReply {
    /// Private mutable access is available only with the sole Arc reference.
    storage: Arc<[u8]>,
}
impl ReusableReply {
    /// Precharges storage before admitting records that could fill a protocol's cap.
    /// The existing pending allowance also bounds trusted caller allocations here.
    pub fn new(capacity: usize) -> Result<Self, &'static str> {
        let length = capacity
            .checked_add(size_of::<u64>())
            .ok_or("reply capacity overflow")?;
        if capacity == 0 || length > super::PENDING_BYTE_ALLOWANCE {
            return Err("reply buffer exceeds pending allowance");
        }
        Ok(Self {
            storage: vec![0; length].into(),
        })
    }

    /// Actual shared allocation plus Arc's two reference counters; inline owner
    /// fields are charged by the caller's own runtime/table accounting.
    pub fn allocated_bytes(&self) -> usize {
        self.storage.len() + 2 * size_of::<usize>()
    }

    /// Payload capacity excludes the fixed C receipt prefix.
    pub fn capacity(&self) -> usize {
        self.storage.len() - size_of::<u64>()
    }

    /// No writer or callback can still observe bytes when this returns true.
    pub fn available(&self) -> bool {
        Arc::strong_count(&self.storage) == 1
    }

    /// Reservation runs only after unique access and payload bounds are proven.
    /// Failure consumes no receipt or sequence and leaves all bytes unchanged.
    pub(crate) fn prepare(
        &mut self,
        bytes: &[u8],
        reserve: impl FnOnce(usize) -> Option<ReplyTicket>,
        notify: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Option<(ReplyTicket, ReplyLease)> {
        if bytes.is_empty() || bytes.len() > self.capacity() {
            return None;
        }
        let charge = self.allocated_bytes();
        let storage = Arc::get_mut(&mut self.storage)?;
        let ticket = reserve(charge)?;
        let prefix = size_of::<u64>();
        storage[..prefix].copy_from_slice(&ticket.id().to_le_bytes());
        storage[prefix..prefix + bytes.len()].copy_from_slice(bytes);
        Some((
            ticket,
            ReplyLease {
                storage: Some(Arc::clone(&self.storage)),
                notify,
                used: prefix + bytes.len(),
            },
        ))
    }
}

/// Shared immutable reply storage; no host may observe unused capacity.
#[derive(Clone)]
pub struct ReplyLease {
    /// Exactly the precharged allocation, shared by producer and transport.
    storage: Option<Arc<[u8]>>,
    /// Last callback release wakes an optional handler waiting for unique storage.
    notify: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Prefix plus actual wire bytes, never the unused/stale suffix.
    used: usize,
}
impl ReplyLease {
    /// C callbacks borrow the receipt prefix and payload directly, with no copy.
    pub fn framed(&self) -> &[u8] {
        self.storage
            .as_deref()
            .and_then(|bytes| bytes.get(..self.used))
            .unwrap_or(&[])
    }
    /// PTY and WebSocket writers send protocol bytes only, never the C prefix.
    pub fn payload(&self) -> &[u8] {
        self.storage
            .as_deref()
            .and_then(|bytes| bytes.get(size_of::<u64>()..self.used))
            .unwrap_or(&[])
    }
}

/// Ordinary replies own exact bytes; scoped protocols may supply a precharged lease.
pub enum ReplyBytes {
    /// Default VT buffers transfer one exact-sized payload allocation.
    Owned(Box<[u8]>),
    /// The optional handler already charged this immutable allocation to its cap.
    Precharged(ReplyLease),
}
impl ReplyBytes {
    /// Foreign hosts retain this lease until actual writer completion.
    pub fn lease(&self) -> Option<ReplyLease> {
        match self {
            Self::Owned(_) => None,
            Self::Precharged(lease) => Some(lease.clone()),
        }
    }
    /// Callback adapters needing owned bytes preserve their existing Vec contract.
    pub fn into_vec(self) -> Vec<u8> {
        match self {
            Self::Owned(bytes) => bytes.into_vec(),
            Self::Precharged(lease) => lease.payload().to_vec(),
        }
    }
}
impl std::ops::Deref for ReplyBytes {
    /// Used protocol bytes exclude the C receipt prefix.
    type Target = [u8];
    /// Both storage forms expose exactly the used protocol payload.
    fn deref(&self) -> &[u8] {
        match self {
            Self::Owned(bytes) => bytes,
            Self::Precharged(lease) => lease.payload(),
        }
    }
}

/// Immutable host borrows can outlive a writer acknowledgement. Their final
/// release must also wake the owner; otherwise a producer could wait forever
/// after observing a still-borrowed buffer during the earlier completion.
impl Drop for ReplyLease {
    fn drop(&mut self) {
        // Release storage before notification so an immediately running owner
        // can observe uniqueness. Notify on each release, avoiding count races.
        self.storage.take();
        if let Some(notify) = self.notify.take() {
            notify();
        }
    }
}

impl AsRef<[u8]> for ReplyLease {
    /// Bridge queues retain the lease itself while sending only used wire bytes.
    fn as_ref(&self) -> &[u8] {
        self.payload()
    }
}
