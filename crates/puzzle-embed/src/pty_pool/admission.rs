//! Host output enters the existing fixed pool without blocking or allocating.
//! Wake tokens share the data inbox, so an available empty buffer does not imply
//! a free inbox slot. Failed publication retains that exact buffer for a retry;
//! neither bytes nor pool storage can disappear under control-channel traffic.
//!
//! A payload is either empty, retained by the producer, queued, or currently
//! being parsed. The pool's original allocations move between those owners;
//! admission creates no extra payload when all 64 buffers are outstanding.
//! The caller keeps the unaccepted suffix, including a chunk whose queue send
//! failed. Retaining its allocation does not transfer ownership of those bytes.
//!
//! Queue capacity and byte credit are distinct: control wakes occupy inbox
//! nodes but consume no payload. Both channels therefore use nonblocking reads
//! or sends on this host path. Consumer disconnection is terminal, whereas an
//! empty channel means temporary backpressure and returns the accepted prefix.
//! Pending accounting precedes publication so an immediately running consumer
//! cannot subtract bytes before the producer adds them. A rejected send rolls
//! that accounting back before its buffer becomes eligible for the next retry.
use super::*;

/// Host admission is an alternative producer, never concurrent with a PTY reader.
impl Producer {
    /// Copies the accepted prefix; zero is backpressure, not EOF or data loss.
    pub(crate) fn admit(&mut self, bytes: &[u8]) -> Result<usize, String> {
        let mut accepted = 0;
        while accepted < bytes.len() {
            // Reuse a payload rejected by a full queue before taking another allocation.
            let mut chunk = match self
                .spare
                .take()
                .map(Ok)
                .unwrap_or_else(|| self.empty.try_recv())
            {
                Ok(chunk) => chunk,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("output consumer closed".into());
                }
            };
            let count = BUFFER_SIZE.min(bytes.len() - accepted);
            chunk.storage[..count].copy_from_slice(&bytes[accepted..accepted + count]);
            chunk.used = count;
            #[cfg(any(debug_assertions, feature = "lock-metrics"))]
            {
                chunk.ready = Some(Instant::now());
            }
            // Account before send: the consumer can run as soon as the node is visible.
            let total = self.pending.fetch_add(count, Ordering::AcqRel) + count;
            match self.full.try_send(DataEvent::Chunk(chunk)) {
                Ok(()) => {
                    self.peak.fetch_max(total, Ordering::AcqRel);
                    accepted += count;
                }
                Err(error) => {
                    self.pending.fetch_sub(count, Ordering::AcqRel);
                    match error {
                        // The caller still owns this prefix and must supply it again.
                        mpsc::TrySendError::Full(DataEvent::Chunk(chunk)) => {
                            self.spare = Some(chunk);
                            break;
                        }
                        mpsc::TrySendError::Disconnected(_) => {
                            return Err("output consumer closed".into());
                        }
                        // Only a Chunk was sent; this variant is unreachable by construction.
                        mpsc::TrySendError::Full(DataEvent::Wake) => {
                            return Err("invalid admission ownership".into());
                        }
                    }
                }
            }
        }
        Ok(accepted)
    }
}

/// Credit notifications carry no data and cannot access the Session or host UI.
impl Consumer {
    /// Installs the sole worker-safe host wakeup used when storage becomes reusable.
    pub(crate) fn notify_returned(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        self.returned = Some(wake);
    }
}
