//! Fixed PTY read-buffer pool with blocking backpressure (D-29 T2).
//!
//! Sixty-four 64 KiB vectors are allocated once before the child starts.
//! The drain thread receives an empty vector, blocks in `Read`, then sends
//! the filled vector to the parser side. The consumer returns it only after
//! every byte has been parsed. With no empty vector the drain stops reading,
//! letting the kernel PTY buffer block the writer instead of dropping data.
//!
//! A buffer can arrive partly filled because macOS PTYs often return about
//! one KiB per read. The pool still bounds *ownership* to four MiB. The
//! consumer exposes 16 KiB slices so the owner can check priority controls
//! between slices without copying bytes or losing an unfinished buffer.
//! Channels use `std::sync::mpsc::sync_channel`; their slots are bounded,
//! and only the preallocated vectors hold PTY data in userspace.
//! Pool return/receive timers observe the parser’s channel handoffs separately
//! from VT work. Their cumulative totals let a host subtract a baseline without
//! storing individual kernel reads or allocating a per-read diagnostic record.
//! This is observational only: channel capacity and the 4-MiB bound are unchanged.
use std::{
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
};

use crate::pipeline_metrics::Metrics;
#[cfg(any(debug_assertions, feature = "lock-metrics"))]
use crate::pipeline_metrics::Stage;
#[cfg(any(debug_assertions, feature = "lock-metrics"))]
use std::time::Instant;

/// Nonblocking host admission shares exactly the same payload ownership.
mod admission;

/// Fixed number of buffers in one live terminal's PTY pipeline.
pub const POOL_COUNT: usize = puzzle_term::platform::reply::PENDING_BYTE_ALLOWANCE / BUFFER_SIZE;
/// Maximum bytes in each reusable read buffer.
pub const BUFFER_SIZE: usize = 65_536;
/// Maximum parse turn before checking controls, one quarter of a pool buffer.
pub const SLICE_SIZE: usize = 16_384;

/// A vector whose allocation survives every producer/consumer transfer.
pub struct Chunk {
    /// Fixed-length storage passed directly to a blocking `Read` call.
    storage: Vec<u8>,
    /// Bytes actually received; PTY reads often fill only a small prefix.
    used: usize,
    /// One timestamp follows this completed OS read through the bounded queue.
    #[cfg(any(debug_assertions, feature = "lock-metrics"))]
    ready: Option<Instant>,
}

/// Data ownership and idle wake tokens share the one bounded drain inbox (D-50).
enum DataEvent {
    /// One of the preallocated payloads; no new byte allocation is permitted.
    Chunk(Chunk),
    /// A control message is waiting on its separate, prioritized channel.
    Wake,
}

/// Cloneable nonblocking notification, with no access to Session or the pool receiver.
#[derive(Clone)]
pub struct DataWake(SyncSender<DataEvent>);
impl DataWake {
    /// A full inbox already wakes its owner, so a redundant token can be discarded.
    pub fn notify(&self) {
        // Full is harmless: the owner checks controls before consuming any queued bytes.
        // Disconnection means shutdown; neither outcome makes the input sender wait.
        // A token owns no pool buffer and contributes no bytes to the backlog.
        let _ = self.0.try_send(DataEvent::Wake);
    }
}

/// Control senders may survive the drain; EOF therefore cannot rely on disconnection.
impl Drop for Producer {
    /// Publishes EOF behind the final read without discarding a queued payload.
    fn drop(&mut self) {
        // Only the drain owns Producer; this edge has one producer and one occurrence.
        self.finished.store(true, Ordering::Release);
        // Waking is nonblocking even when all payload slots are occupied. In that
        // case queued data already wakes the consumer; it observes EOF after draining.
        let _ = self.full.try_send(DataEvent::Wake);
    }
}

/// Drain-side ownership of empty buffers and the full-buffer sender.
pub struct Producer {
    /// A missing empty buffer blocks the OS reader, applying backpressure.
    empty: Receiver<Chunk>,
    /// A full data inbox may contain wake tokens; retain the unsent payload here.
    spare: Option<Chunk>,
    /// Filled buffers move without allocating or cloning their payload.
    full: SyncSender<DataEvent>,
    /// EOF survives control senders without a blocking destructor.
    finished: Arc<AtomicBool>,
    /// Unparsed bytes include full and currently consumed buffers.
    pending: Arc<AtomicUsize>,
    /// Maximum unparsed count observed since pool construction.
    peak: Arc<AtomicUsize>,
    /// The pool and parser share observations only in an instrumented build.
    #[cfg(any(debug_assertions, feature = "lock-metrics"))]
    metrics: Arc<Metrics>,
}

/// Parser-side ownership of filled buffers and the return channel.
pub struct Consumer {
    /// The parser can poll this without blocking a UI thread in T2.
    full: Receiver<DataEvent>,
    /// Control senders wake this inbox without sharing its receiver.
    wake: SyncSender<DataEvent>,
    /// EOF is explicit because control senders keep the data channel alive.
    closed: bool,
    /// The drain marks this after its final payload send, before waking idle consumption.
    finished: Arc<AtomicBool>,
    /// Returning a fully consumed buffer wakes a backpressured drain.
    empty: SyncSender<Chunk>,
    /// A partially parsed buffer keeps its unread suffix in pool storage.
    current: Option<Chunk>,
    /// Host sources retry credit admission after a returned buffer, without polling.
    returned: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Next unread byte inside `current`, never larger than `used`.
    offset: usize,
    /// Exposes total unparsed bytes for the four-MiB acceptance gate.
    pending: Arc<AtomicUsize>,
    /// Peak is sampled without locking the parser or drain threads.
    peak: Arc<AtomicUsize>,
    /// Cold counters also serve runtime-enabled input diagnostics in any native build.
    metrics: Arc<Metrics>,
}

/// Preallocates every payload before PTY output can enter the application.
pub fn pair() -> (Producer, Consumer) {
    // Two bounded queues account for every buffer even during a partial parse.
    let (empty_tx, empty_rx) = mpsc::sync_channel(POOL_COUNT);
    let (full_tx, full_rx) = mpsc::sync_channel(POOL_COUNT);
    for _ in 0..POOL_COUNT {
        // The fixed length is intentional: `Read` needs a writable slice
        // without increasing Vec length on each OS callback.
        let chunk = Chunk {
            storage: vec![0; BUFFER_SIZE],
            used: 0,
            #[cfg(any(debug_assertions, feature = "lock-metrics"))]
            ready: None,
        };
        // Exactly POOL_COUNT slots were reserved, so seeding cannot fail
        // unless a channel endpoint was unexpectedly dropped at startup.
        let _ = empty_tx.send(chunk);
    }
    let metrics = Arc::new(Metrics::default());
    // Backlog counters track bytes, unlike queue lengths or vector capacities.
    let pending = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicBool::new(false));
    (
        Producer {
            empty: empty_rx,
            spare: None,
            full: full_tx.clone(),
            finished: finished.clone(),
            pending: Arc::clone(&pending),
            peak: Arc::clone(&peak),
            #[cfg(any(debug_assertions, feature = "lock-metrics"))]
            metrics: Arc::clone(&metrics),
        },
        Consumer {
            full: full_rx,
            wake: full_tx,
            closed: false,
            finished,
            empty: empty_tx,
            current: None,
            returned: None,
            offset: 0,
            pending,
            peak,
            metrics,
        },
    )
}

/// The drain moves each buffer once; every byte stays owned until consumption.
impl Producer {
    /// Reads one OS chunk, returning false at EOF or after a closed consumer.
    pub fn read_once(&mut self, reader: &mut dyn Read) -> Result<bool, String> {
        // Try once so the diagnostic distinguishes true backpressure from
        // the small channel cost of taking an already available empty vector.
        let chunk = match self.empty.try_recv() {
            Ok(chunk) => Some(chunk),
            Err(mpsc::TryRecvError::Disconnected) => None,
            Err(mpsc::TryRecvError::Empty) => {
                #[cfg(any(debug_assertions, feature = "lock-metrics"))]
                let started = Instant::now();
                let chunk = self.empty.recv().ok();
                #[cfg(any(debug_assertions, feature = "lock-metrics"))]
                self.metrics.record(Stage::Exhausted, started.elapsed());
                chunk
            }
        };
        // A closed parser ends draining without allocating a replacement buffer.
        let Some(mut chunk) = chunk else {
            return Ok(false);
        };
        #[cfg(any(debug_assertions, feature = "lock-metrics"))]
        let started = Instant::now();
        // EINTR retries the same writable allocation rather than losing its ownership.
        let count = loop {
            match reader.read(&mut chunk.storage) {
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(format!("PTY read: {error}")),
                Ok(count) => break count,
            }
        };
        // EOF terminates the drain; no empty payload is sent to the parser.
        if count == 0 {
            return Ok(false);
        }
        #[cfg(any(debug_assertions, feature = "lock-metrics"))]
        {
            // Stamp after the read so queue residence excludes blocking kernel time.
            self.metrics.record(Stage::Read, started.elapsed());
            self.metrics.read_size(count);
            chunk.ready = Some(Instant::now());
        }
        // Unwritten vector bytes never enter the parser even on a short OS read.
        chunk.used = count;
        // Publish byte ownership before sending the corresponding filled chunk.
        let total = self.pending.fetch_add(count, Ordering::AcqRel) + count;
        self.peak.fetch_max(total, Ordering::AcqRel);
        // The sender blocks when the parser owns all sixty-four buffers;
        // its read cursor cannot advance while the child keeps writing.
        Ok(self.full.send(DataEvent::Chunk(chunk)).is_ok())
    }
}

/// A borrowed suffix survives priority-control turns without copying its bytes.
impl Consumer {
    /// Waits for a filled buffer when idle without polling a UI thread.
    #[cfg(any(test, feature = "test-support"))]
    pub fn next_slice_blocking(&mut self) -> Option<&[u8]> {
        while self.current.is_none() && !self.closed {
            self.wait_data();
        }
        self.next_slice()
    }

    /// Wakes on bytes or a control token; the owner rechecks controls before parsing.
    pub fn wait_data(&mut self) {
        if self.current.is_none() && !self.closed {
            // Observe EOF before polling: its release follows every final payload send.
            let finished = self.finished.load(Ordering::Acquire);
            // Queued final payloads must be drained before EOF is acted on.
            if let Ok(event) = self.full.try_recv() {
                self.accept_data(event);
                return;
            }
            if finished {
                self.closed = true;
                return;
            }
            match self.full.recv() {
                Ok(event) => self.accept_data(event),
                Err(_) => self.closed = true,
            }
        }
    }

    /// Applies one ownership transfer without copying the fixed payload.
    fn accept_data(&mut self, event: DataEvent) {
        match event {
            DataEvent::Chunk(chunk) => {
                self.current = Some(chunk);
                self.offset = 0;
            }
            // A wake carries no bytes; controls have their own priority inbox.
            DataEvent::Wake => {}
        }
    }

    /// Control sends never block behind full output; fullness itself guarantees wakefulness.
    pub fn wake_handle(&self) -> DataWake {
        DataWake(self.wake.clone())
    }

    /// True only after the drain's final bytes have been offered to the owner.
    pub fn closed(&self) -> bool {
        self.closed && self.current.is_none()
    }

    /// Borrows at most 16 KiB, keeping an unfinished pool buffer in place.
    pub fn next_slice(&mut self) -> Option<&[u8]> {
        if self.current.is_none() {
            #[cfg(any(debug_assertions, feature = "lock-metrics"))]
            let started = Instant::now();
            let finished = self.finished.load(Ordering::Acquire);
            while let Ok(event) = self.full.try_recv() {
                self.accept_data(event);
                if self.current.is_some() || self.closed {
                    break;
                }
            }
            #[cfg(any(debug_assertions, feature = "lock-metrics"))]
            self.metrics.record(Stage::PoolReceive, started.elapsed());
            // EOF is read only after the data queue was drained without finding a chunk.
            if self.current.is_none() && finished {
                self.closed = true;
            }
            self.offset = 0;
        }
        #[cfg(any(debug_assertions, feature = "lock-metrics"))]
        if let Some(at) = self.current.as_mut().and_then(|chunk| chunk.ready.take()) {
            self.metrics.record(Stage::Queue, at.elapsed());
        }
        // A missing chunk means the producer has no new bytes ready this turn.
        let chunk = self.current.as_ref()?;
        // Slices respect both the slice limit and the actual received prefix.
        let end = (self.offset + SLICE_SIZE).min(chunk.used);
        Some(&chunk.storage[self.offset..end])
    }

    /// Returns parsed bytes to the pool only when their buffer is exhausted.
    pub fn consume(&mut self, count: usize) -> Result<(), String> {
        let Some(chunk) = self.current.as_ref() else {
            return Err("PTY pool has no current buffer".into());
        };
        // Only a nonempty suffix already offered by this slice can be consumed.
        if count == 0 || self.offset + count > chunk.used {
            return Err("PTY pool consumed outside buffer".into());
        }
        self.offset += count;
        self.pending.fetch_sub(count, Ordering::AcqRel);
        if self.offset == chunk.used {
            let Some(chunk) = self.current.take() else {
                return Err("PTY pool current buffer vanished".into());
            };
            // The consumer holds one of the fixed buffers, so the empty
            // channel always has space for its return after parsing.
            // After EOF the producer has dropped its receiver. Returning
            // the final parsed chunk is then unnecessary, not a data loss.
            #[cfg(any(debug_assertions, feature = "lock-metrics"))]
            let started = Instant::now();
            let restored = self.empty.send(chunk).is_ok();
            // Only returned storage creates admission credit; slice consumption alone does not.
            if restored && let Some(wake) = &self.returned {
                wake();
            }
            #[cfg(any(debug_assertions, feature = "lock-metrics"))]
            self.metrics.record(Stage::PoolReturn, started.elapsed());
            self.offset = 0;
        }
        Ok(())
    }

    /// Shares the same bounded stage owner with the Session's parser/snapshot path.
    pub fn metrics_handle(&self) -> Arc<Metrics> {
        Arc::clone(&self.metrics)
    }

    /// Reports byte backlog rather than the pool's fixed allocation size.
    pub fn pending_bytes(&self) -> usize {
        self.pending.load(Ordering::Acquire)
    }

    /// Shares the byte count with the live perf logger and storm probe.
    pub fn pending_handle(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.pending)
    }

    /// Exposes the fixed pool's exact high-water mark to the storm probe.
    pub fn peak_handle(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.peak)
    }

    /// Reports the largest observed backlog for the flood budget.
    pub fn peak_bytes(&self) -> usize {
        self.peak.load(Ordering::Acquire)
    }
}

#[cfg(test)]
/// Byte-order, backpressure and allocation checks for fixed handoff.
mod tests;
