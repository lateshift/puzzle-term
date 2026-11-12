//! Local, neutral M16 producers stay under target and never enter shipped assets.
//!
//! The benchmark is the user's existing sibling checkout, not a downloaded crate
//! or copied third-party fixture. Its release executable supplies the actual
//! Unicode workload. The separate input reader stamps every key independently.
use std::{fs, path::PathBuf};

/// Writes the actual thirty-second benchmark and thirty-second frozen-screen producer.
pub(super) fn pressure() -> Result<String, String> {
    // Workspace ownership determines the sibling path without editing that checkout.
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let sibling = root
        .parent()
        .ok_or("workspace has no parent")?
        .join("vtebench");
    // Explicit environment paths support another approved development checkout.
    let binary = std::env::var_os("PUZZLE_VTEBENCH")
        .map(PathBuf::from)
        .unwrap_or_else(|| sibling.join("target/release/vtebench"));
    // Require the actual Unicode benchmark directory, including every original scalar.
    let corpus = std::env::var_os("PUZZLE_VTEBENCH_UNICODE")
        .map(PathBuf::from)
        .unwrap_or_else(|| sibling.join("benchmarks/unicode"));
    // Missing tools need explicit user arrangement; this check never installs them.
    if !binary.is_file() || !corpus.is_dir() {
        return Err(
            "M16 requires the existing release vtebench executable and Unicode corpus".into(),
        );
    }
    // JSON strings safely quote paths as Python literals without shell interpolation.
    let script = PRESSURE
        .replace("BENCH_BINARY", &serde_json::json!(binary).to_string())
        .replace("BENCH_CORPUS", &serde_json::json!(corpus).to_string())
        // Receipt directories are private; the benchmark report keeps its gate path.
        .replace(
            "BENCH_REPORT",
            &serde_json::json!(root.join("target/m16-2/puzzle-vtebench.dat")).to_string(),
        );
    // The generated wrapper is our own evidence code, not a vendored benchmark copy.
    fs::create_dir_all("target/m16-2").map_err(|e| e.to_string())?;
    let path = root.join("target/m16-2/pressure-producer.py");
    // Paths are already quoted into Python literals; argv remains subprocess structured.
    fs::write(&path, script).map_err(|e| e.to_string())?;
    // An absolute script path survives the native child shell's working directory.
    Ok(path.to_string_lossy().into_owned())
}

/// The real workloads are the previously authorized neutral local command fixtures.
pub(super) fn workloads() -> Result<String, String> {
    // Retain the existing local corpus and installed-app fixtures unchanged.
    let path = std::env::var_os("PUZZLE_M16_WORKLOAD_PRODUCER")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/workloads/producer.py"));
    // Missing evidence is a failure, never a silent synthetic substitute for real apps.
    fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

/// The title handshake keeps setup outside the full benchmark interval.
/// The first actual frozen presentation must contain all 5,120 covered syllables.
/// Raw mode and a concurrent reader keep child input independent of its output.
/// No artificial producer pauses or smaller Unicode corpus improve the reported fps.
/// Parent and child keep distinct histories: key.txt is the latest compatibility
/// stamp, while .samples is the exact ordered receipt set used by the count gate.
/// Atomic replacement publishes the complete set; a snapshot during a write must
/// never turn a partially written JSON document into a spurious missing key.
const PRESSURE: &str = r#"import os, sys, time, threading, subprocess, json
from pathlib import Path
os.write(1,b'\x1b]2;unicode-ready\x07')
sys.stdin.buffer.read(1)
def input_reader():
    received = []
    while True:
        if not sys.stdin.buffer.read(1):
            return
        received.append(time.time_ns())
        Path(sys.argv[1]).write_text(str(received[-1]))
        # Each argv path belongs to exactly one probe, so this sibling temporary
        # file cannot replace another child's journal during concurrent launches.
        # Publish a complete journal atomically; an observer never reads half JSON.
        journal = Path(sys.argv[1]+'.samples')
        pending = journal.with_suffix('.pending')
        pending.write_text(json.dumps(received))
        pending.replace(journal)
threading.Thread(target=input_reader,daemon=True).start()
subprocess.run([BENCH_BINARY,'--benchmarks',BENCH_CORPUS,'--max-secs','30','--dat',BENCH_REPORT],check=True)
os.write(1,b'\x1bc\x1b[?25l')
os.write(1,''.join(chr(c) for c in range(0xC000,0xD400)).encode())
os.write(1,b'\x1b]2;unicode-frozen\x07')
time.sleep(40)
"#;
