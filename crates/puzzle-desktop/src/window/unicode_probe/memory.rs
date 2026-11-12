//! Development-only process memory snapshots separate RSS from virtual mappings.
//!
//! Query exactly this process ID, never enumerate other applications. `ps` reports
//! kilobytes on the dev Mac; both values are converted to bytes and retained as
//! independent observations. Missing platform support reports an explicit error.
use serde_json::{Value, json};

/// Samples outside timed frame turns; mapped font bytes are reported by their owner.
pub(super) fn sample() -> Value {
    let pid = std::process::id().to_string();
    let result = std::process::Command::new("ps")
        .args(["-o", "rss=,vsz=", "-p", &pid])
        .output();
    let Ok(result) = result else {
        return json!({"error":"process memory tool unavailable"});
    };
    // Parse the two numeric columns only; command names and other processes are absent.
    let values: Vec<u64> = String::from_utf8_lossy(&result.stdout)
        .split_whitespace()
        .filter_map(|value| value.parse().ok())
        .collect();
    if !result.status.success() || values.len() != 2 {
        return json!({"error":"process memory sample invalid"});
    }
    // Virtual mappings and resident pages remain separate byte measurements.
    json!({"pid":pid,"resident_bytes":values[0] * 1024,"virtual_bytes":values[1] * 1024})
}
