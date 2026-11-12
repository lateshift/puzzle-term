//! Retains the five longest parser intervals with their paired thread CPU times.
//! This fixed array has one parser writer and is read only after a finite cohort.
//! No lock, allocation or application byte storage enters interval recording.
//! Both clocks stop before journal insertion, so reporting overhead is excluded.
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

/// Descending wall-time order makes all retained outliers visible in the report.
#[derive(Default)]
pub(in crate::pipeline_metrics) struct Tail {
    /// Five fixed records are independent of output volume and scrollback size.
    pairs: [Pair; 5],
}

/// One inseparable wall/CPU observation, published by the sole parser writer.
#[derive(Default)]
struct Pair {
    /// Wall duration determines ordering without applying the withdrawn lock budget.
    wall: AtomicU64,
    /// CPU duration excludes preemption; MAX marks an unsupported clock.
    cpu: AtomicU64,
}

/// Insertions shift at most four fixed pairs and never change parse scheduling.
impl Tail {
    /// Keeps the five largest feeds without dropping an earlier larger outlier.
    pub(in crate::pipeline_metrics) fn record(&self, times: (Duration, Option<Duration>)) {
        // The source duration remains nanosecond-resolution until report formatting.
        // Saturation prevents a pathological uptime from truncating a large sample.
        let wall = times.0.as_nanos().min(u128::from(u64::MAX)) as u64;
        // Only a sample larger than an existing slot enters the tail. Ties keep
        // their chronological predecessor; every run still retains all five maxima.
        // The common smaller interval requires only five relaxed loads.
        let Some(index) = self
            .pairs
            .iter()
            .position(|p| wall > p.wall.load(Ordering::Relaxed))
        else {
            return;
        };
        // Move from the cold end backward, avoiding overwrite of the next source.
        // Atomic fields keep the shared Metrics owner Send/Sync without a mutex.
        // Reports are cohort-final: a live read could observe a partial insertion.
        for i in (index + 1..self.pairs.len()).rev() {
            // One writer preserves each pair while moving it to its colder slot.
            self.pairs[i].wall.store(
                self.pairs[i - 1].wall.load(Ordering::Relaxed),
                Ordering::Relaxed,
            );
            self.pairs[i].cpu.store(
                self.pairs[i - 1].cpu.load(Ordering::Relaxed),
                Ordering::Relaxed,
            );
        }
        // Store the two values from one frozen interval in the same selected slot.
        // MAX is an absence sentinel, distinct from a genuinely tiny CPU duration.
        self.pairs[index].wall.store(wall, Ordering::Relaxed);
        self.pairs[index].cpu.store(
            times.1.map_or(u64::MAX, |v| {
                v.as_nanos().min(u128::from(u64::MAX - 1)) as u64
            }),
            Ordering::Relaxed,
        );
    }

    /// Formats pairs only after the producer stops, never on the hot owner path.
    #[cfg(any(test, feature = "lock-metrics"))]
    pub(in crate::pipeline_metrics) fn report(&self) -> serde_json::Value {
        // Formatting owns its bounded result outside Session and after observation.
        // Empty initialization slots are omitted when fewer than five feeds occurred.
        let rows: Vec<_> = self
            .pairs
            .iter()
            .filter_map(|pair| {
                let wall = pair.wall.load(Ordering::Relaxed);
                if wall == 0 {
                    return None;
                }
                let cpu = pair.cpu.load(Ordering::Relaxed);
                Some(serde_json::json!({"wall_ms":wall as f64/1e6,
                "thread_cpu_ms":(cpu != u64::MAX).then_some(cpu as f64/1e6)}))
            })
            .collect();
        serde_json::Value::Array(rows)
    }
}
