//! Optional long-run atlas evidence leaves the existing six-second gate unchanged.
//!
//! The actual benchmark ends through its frozen-screen OSC title. Every flood
//! submission and boundary page eviction remains measured, followed by thirty
//! seconds without output before checking final glyph identities and GPU ink.
//! Frame durations stay in submission order so slow work can be located rather
//! than hidden by an average. The observer never changes the refresh scheduler.
use super::super::live::LiveWindow;
use crate::window::live::UnicodePressure;
use std::time::{Duration, Instant};

/// Counters belong only to the explicitly selected D-39 development workload.
pub(super) struct Pressure {
    /// First observation of the frozen-screen title closes the complete flood interval.
    pub(super) frozen: Option<Instant>,
    /// Worst complete redraw includes page eviction, snapshot, uploads and acquisition.
    pub(super) worst_ms: f64,
    /// Successful between-frame page evictions during the benchmark, never redraw requests.
    pub(super) evictions: u64,
    /// Maximum charged vector capacity remains subject to the unchanged 48 MiB budget.
    pub(super) bytes: usize,
    /// Complete redraw durations correspond to every retained flood submission.
    pub(super) frame_ms: Vec<f64>,
    /// Every frozen submission retains its readiness and publication counters.
    pub(super) frozen_frames: Vec<serde_json::Value>,
    /// First complete built image, measured at successful native submission.
    pub(super) ink_ms: Option<f64>,
    /// Worker counters at freeze distinguish startup indexing from screen recovery.
    pub(super) at_freeze: serde_json::Value,
}

impl Pressure {
    /// Normal windows and ordinary acceptance keep their previous observer lifetime.
    pub(super) fn enabled() -> Option<Self> {
        // The longer observer is opt-in and does not alter ordinary acceptance duration.
        std::env::var_os("PUZZLE_UNICODE_PRESSURE").map(|_| Self {
            frozen: None,
            worst_ms: 0.0,
            evictions: 0,
            bytes: 0,
            frame_ms: Vec::with_capacity(2048),
            frozen_frames: Vec::new(),
            ink_ms: None,
            at_freeze: serde_json::Value::Null,
        })
    }

    /// Freezes flood counters at the actual producer boundary, then waits without output.
    pub(super) fn complete(&mut self, live: &LiveWindow, now: Instant) -> bool {
        if self.frozen.is_none() && live.title == "unicode-frozen" {
            // Latch once; a repeated title observation must not restart the frozen interval.
            self.frozen = Some(now);
            // Snapshot cumulative worker counters so recovery work can be subtracted.
            self.at_freeze = live.glyph_timing();
        }
        self.frozen
            .is_some_and(|at| now.duration_since(at) >= Duration::from_secs(30))
    }

    /// Samples the ordinary redraw duration and atlas counters before the frozen phase.
    pub(super) fn presented(&mut self, elapsed_ms: f64, live: &LiveWindow) {
        let now = Instant::now();
        self.complete(live, now);
        if let Some(at) = self.frozen {
            // Parsing can reach the title while an older snapshot is still being rendered.
            // That complete older image is not a presentation of the frozen Hangul screen.
            if !live.frozen_snapshot() {
                return;
            }
            let ms = now.duration_since(at).as_secs_f64() * 1000.0;
            // Missing evidence is incomplete, never interpreted as a resolved screen.
            let status = live.glyph_status.unwrap_or([0, 0]);
            let metrics = live.text_context.completion_metrics().unwrap_or_default();
            // Count final ink handles from either backend, excluding provisional boxes.
            if status == [5120, 5120] && self.ink_ms.is_none() {
                self.ink_ms = Some(ms);
            }
            // Retain each recovery presentation to show progress or a publication stall.
            self.frozen_frames.push(serde_json::json!({
                "ms": ms, "cells": status[0], "ready": status[1],
                "redraw_ms": elapsed_ms, "publish_ink": metrics.ink,
                "publish_turns": metrics.turns, "publish_ms": metrics.nanos as f64 / 1e6,
                "publish_worst_ms": metrics.worst_nanos as f64 / 1e6,
                "evictions": live.text_context.vector_pressure().0,
            }));
        }
        if self.frozen.is_none() {
            self.worst_ms = self.worst_ms.max(elapsed_ms);
            // Keep slow samples rather than reporting only an average flood rate.
            self.frame_ms.push(elapsed_ms);
            let (evictions, bytes) = live.text_context.vector_pressure();
            self.evictions = evictions;
            self.bytes = self.bytes.max(bytes);
        }
    }
}
