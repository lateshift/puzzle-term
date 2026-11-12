//! One-second counters for a real window under PTY flood and live resize.
//!
//! The headless storm cannot reproduce macOS's modal resize loop. These
//! counters use actual winit callbacks and successful surface submissions.
//! A user can compare the old architecture and D-29 while dragging the
//! same workload; no visual judgment is inferred from a headless run.
//! Logging occurs once per active second so it does not add per-cell work.
//! Parsed bytes come from the worker's monotonic counter, so reading them
//! does not acquire Session and change the contention being observed.
//! Pending bytes come from the fixed pool and include buffers the parser
//! has not fully consumed; this makes a full pool visible in the log.
//! Frame counts retain their existing successful GPU-submission meaning.
//! Actual CA assignments are independently traced by the native display path.
//! A `RedrawRequested` callback without damage is deliberately excluded.
//! Main busy time measures work inside one host callback, including native
//! display waits. Silence between callbacks is deliberately excluded: holding
//! the mouse still does not make the application busy.
//! The first resize after an idle shell starts a new frame-gap window;
//! preceding idle time never contributes to callback busy duration.
//! A one-second grace period includes the final throttled PTY resize callback.
//! Each log carries an `elapsed_ms` field because the event loop may be
//! blocked past the nominal report boundary during a modal resize.
use std::time::{Duration, Instant};

/// Accumulates one report interval on the winit thread only.
pub struct PerfLog {
    /// Start of the current one-second sample.
    interval: Instant,
    /// Recent resize activity classifies frame gaps for one trailing second.
    resizing_until: Option<Instant>,
    /// Last successful presentation for interval-gap measurement.
    last_frame: Option<Instant>,
    /// Parsed byte counter at the previous report boundary.
    prior_bytes: u64,
    /// Successful swapchain submissions in this interval.
    frames: u32,
    /// Largest gap between successful frames in this interval.
    max_frame_gap: Duration,
    /// Longest completed host callback, independent of inter-event silence.
    max_main_busy: Duration,
    /// CPU of that same longest callback; absent when the host cannot observe it.
    max_main_busy_cpu: Option<Duration>,
}

impl PerfLog {
    /// Starts timing only after the OS window and Session are ready.
    pub fn new(now: Instant) -> Self {
        Self {
            interval: now,
            resizing_until: None,
            last_frame: None,
            prior_bytes: 0,
            frames: 0,
            max_frame_gap: Duration::ZERO,
            max_main_busy: Duration::ZERO,
            max_main_busy_cpu: None,
        }
    }

    /// Records an actual winit callback before any Session or GPU work.
    pub fn event(&mut self, now: Instant, resized: bool) {
        // Resize activity still classifies frame gaps, but never turns time
        // waiting for the next OS event into claimed main-thread work.
        if resized {
            self.resizing_until = Some(now + Duration::from_secs(1));
        }
    }

    /// A completed callback contributes only its own measured residence time.
    pub fn busy(&mut self, elapsed: Duration) {
        self.busy_observed(elapsed, None);
    }

    /// Pair CPU only with the callback that supplied the maximum wall residence.
    fn busy_observed(&mut self, elapsed: Duration, cpu: Option<Duration>) {
        if elapsed > self.max_main_busy {
            self.max_main_busy = elapsed;
            self.max_main_busy_cpu = cpu;
        }
    }

    /// Counts only a frame actually submitted to the window surface.
    pub fn presented(&mut self, now: Instant) {
        if let Some(last) = self.last_frame {
            let gap = now.duration_since(last);
            // A shell idle for seconds did not drop animation frames;
            // exclude that transition from the active frame-gap maximum.
            if gap < Duration::from_secs(1) || self.resizing_until.is_some_and(|due| now <= due) {
                self.max_frame_gap = self.max_frame_gap.max(gap);
            }
        }
        self.last_frame = Some(now);
        self.frames += 1;
    }

    /// Emits one active interval with parser, frame and resize timing.
    pub fn report(&mut self, now: Instant, parsed_total: u64, pending: usize) {
        let elapsed = now.duration_since(self.interval);
        if elapsed < Duration::from_secs(1) {
            return;
        }
        let bytes = parsed_total.saturating_sub(self.prior_bytes);
        // Explicit millisecond units let the old and new live-window logs
        // be placed side by side without reconstructing timer ticks.
        let max_frame_gap_ms = self.max_frame_gap.as_secs_f64() * 1000.0;
        let max_main_busy_ms = self.max_main_busy.as_secs_f64() * 1000.0;
        let resize_active = self.resizing_until.is_some_and(|due| now <= due);
        tracing::info!(
            target: "puzzle_desktop::perf",
            bytes_parsed = bytes,
            frames_presented = self.frames,
            max_frame_gap_ms,
            max_main_busy_ms,
            max_main_busy_cpu_ms = self.max_main_busy_cpu.map(|v| v.as_secs_f64() * 1000.0),
            pending_bytes = pending,
            resize_active,
            elapsed_ms = elapsed.as_millis(),
            "live-window one-second counters"
        );
        self.interval = now;
        self.prior_bytes = parsed_total;
        self.frames = 0;
        self.max_frame_gap = Duration::ZERO;
        self.max_main_busy = Duration::ZERO;
        self.max_main_busy_cpu = None;
    }
}

/// Injected clocks distinguish callback residence from event delivery gaps.
#[cfg(test)]
mod tests;

/// Shared scalar counters: the host records callbacks, the actor records presentations.
/// The guard ends before control sends, publication handling, builds or GPU work.
#[derive(Clone)]
pub struct Counters(std::sync::Arc<std::sync::Mutex<PerfLog>>);
impl Counters {
    /// Allocates one cold counter owner for the native host and its render driver.
    pub fn new(now: Instant) -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(PerfLog::new(
            now,
        ))))
    }
    /// Marks resize activity before forwarding, without measuring event gaps.
    fn event(&self, now: Instant, resized: bool) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .event(now, resized);
    }
    /// Scoped main-thread handling includes early returns and nested CA waits.
    pub fn callback(&self, resized: bool) -> Callback {
        let started = Instant::now();
        self.event(started, resized);
        Callback {
            counters: self.clone(),
            started,
            cpu: tracing::enabled!(target: "puzzle_desktop::input", tracing::Level::DEBUG)
                .then(crate::thread_cpu::diagnostic_now)
                .flatten(),
            resized,
        }
    }
    /// Native CA display may execute outside a winit callback during tracking.
    pub fn busy(&self, elapsed: Duration) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .busy(elapsed);
    }
    /// Records completed GPU submissions after all frame work has finished.
    pub fn presented(&self, now: Instant) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .presented(now);
    }
    /// Reports scalar samples only, without waiting for any rendering resource.
    pub fn report(&self, now: Instant, parsed: u64, pending: usize) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .report(now, parsed, pending);
    }
}

/// Owned diagnostic guard; it never borrows input state across a callback.
pub struct Callback {
    /// Shared scalars survive even if this event closes its window.
    counters: Counters,
    /// Callback entry, rather than the preceding callback's exit.
    started: Instant,
    /// CPU belongs to this callback thread, excluding OS residence before entry.
    cpu: Option<Duration>,
    /// Distinguish the geometry callback from unrelated main-thread activity.
    resized: bool,
}
impl Drop for Callback {
    /// Sample after our work, never while waiting for another OS event.
    fn drop(&mut self) {
        let wall = self.started.elapsed();
        let cpu = self
            .cpu
            .and_then(|start| crate::thread_cpu::diagnostic_now()?.checked_sub(start));
        self.counters
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .busy_observed(wall, cpu);
        // The dedicated input diagnostic is the only per-callback log destination.
        tracing::trace!(target: "puzzle_desktop::input", event="desktop.main_callback",
            wall_ms=wall.as_secs_f64()*1000.0, cpu_ms=cpu.map(|v| v.as_secs_f64()*1000.0),
            resize=self.resized);
    }
}
