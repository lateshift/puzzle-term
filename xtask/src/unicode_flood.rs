//! A neutral real-PTY Unicode producer is shared by desktop and Chrome acceptance.
//!
//! Generated files stay in target; no font or third-party fixture is vendored.
//! The child reads input concurrently with output and records its receipt clock,
//! so the latency witness measures delivery to a program rather than enqueueing.
use std::{fs, path::PathBuf, process::Command};
/// Generates the authorized equivalent corpus and producer, returning an absolute script path.
pub(crate) fn fixture() -> Result<String, String> {
    // Fixtures are regenerated under target and never change golden references or assets.
    // Both platform runners point at this same corpus and child program.
    let dir = PathBuf::from("target/m11-unicode");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // A reviewer can select the local vtebench file without vendoring third-party bytes.
    // The generated default has the same distinct-scalar count and absent-script pressure.
    let corpus = match std::env::var_os("PUZZLE_UNICODE_CORPUS") {
        Some(path) => fs::read(path).map_err(|e| e.to_string())?,
        // Many supplementary scripts have no installed face; include covered scripts in the header.
        None => (0x10000..0x10000 + 46607)
            .filter_map(char::from_u32)
            .collect::<String>()
            .into_bytes(),
    };
    // Reject invalid/empty fixtures before a raw PTY child can enter its producer loop.
    let text = std::str::from_utf8(&corpus).map_err(|e| e.to_string())?;
    if text.chars().collect::<std::collections::HashSet<_>>().len() < 46607 {
        return Err("Unicode corpus must contain at least 46,607 distinct scalars".into());
    }
    // Validation precedes launch: a malformed corpus must fail the fixture itself,
    // rather than leaving a raw-mode child waiting forever for the start handshake.
    fs::write(dir.join("corpus"), corpus).map_err(|e| e.to_string())?;
    let script = dir.join("producer.py");
    fs::write(&script, PRODUCER).map_err(|e| e.to_string())?;
    // The bridge starts its PTY in the home directory, so a relative script would
    // exercise a missing-file error instead of the intended Unicode producer.
    fs::canonicalize(script)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}
/// Runs the ordinary instrumented native window and validates all retained measurements.
pub(crate) fn native() -> Result<(), String> {
    // Build before opening the timed window; compilation cannot inflate flood samples.
    // The existing lock-metrics feature makes this a normal release presentation path.
    let script = fixture()?;
    crate::commands::quiet(
        "cargo",
        &[
            "build",
            "--release",
            "-p",
            "puzzle-desktop",
            "--features",
            "lock-metrics",
        ],
    )?;
    // A neutral configuration preserves default system fallback and presentation rules.
    // User dotfiles and shell initialization cannot interfere with the raw producer.
    fs::write("target/m11-unicode/config.json", "{}").map_err(|e| e.to_string())?;
    // A crashed or timed-out window cannot leave a previous passing report authoritative.
    let _ = fs::remove_file("target/m11-unicode/native.json");
    // Enable observation only for this child process. The application uses the same
    // winit surface, late snapshot, raw-key adapter and ordered PTY writer as normal.
    let status = Command::new("target/release/puzzle-desktop")
        .args(["run", "--config", "target/m11-unicode/config.json"])
        .env("SHELL", "/bin/sh")
        .env("PUZZLE_UNICODE_PROBE", &script)
        .env("PUZZLE_GLYPH_TIMING", "1")
        .env("PUZZLE_LOG_FILE", "target/m11-unicode/native.log")
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("Unicode native window failed".into());
    }
    // Require a fresh report after successful exit, preserving all submission offsets.
    // An absent report or explicit false verdict is a failure, never an ignored run.
    let report: serde_json::Value = serde_json::from_slice(
        &fs::read("target/m11-unicode/native.json").map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    // The complete JSON exposes long gaps, provisional ink and later source completion.
    // The probe applies unchanged 30-fps and 50-ms limits to the entire interval.
    println!("M11 Unicode native: {report}");
    if report["pass"] != true {
        return Err("Unicode presentation/input acceptance failed".into());
    }
    Ok(())
}
/// Python is the already approved test interpreter; all content and paths are neutral/local.
/// The OSC title handshake keeps setup output outside the measured flood interval.
/// The runner sends its start byte only after beginning its monotonic clock.
/// A separate reader thread records actual child receipt with the machine UTC clock.
/// Raw PTY mode makes the recorded Ctrl+A byte available without waiting for Enter.
/// Output uses chunks of 512 scalars; UTF-8 bytes are never split by inserted headers.
/// The six-second loop traverses the corpus repeatedly, exceeding coverage-cache capacity.
/// Its four-millisecond pause leaves enough output to dirty every display refresh.
/// A scrolling region excludes row one, keeping covered CJK/emoji and absent U+0378 visible.
/// DECSC/DECRC restore the producer cursor around this independent diagnostic header.
/// The final title ends the fixture; no parser summary substitutes for frame evidence.
const PRODUCER: &str = r#"import os, sys, time, threading
from pathlib import Path
corpus = Path(__file__).with_name('corpus').read_text()
os.write(1, b'\x1b]2;unicode-ready\x07')
sys.stdin.buffer.read(1)
def input_reader():
    sys.stdin.buffer.read(1)
    received = time.time_ns()
    if len(sys.argv) > 1:
        Path(sys.argv[1]).write_text(str(received))
    os.write(1, ('\x1b]2;unicode-key-%d\x07' % received).encode())
threading.Thread(target=input_reader, daemon=True).start()
os.write(1, b'\x1b[?25l\x1b[2;20r\x1b[2;1H')
start = time.monotonic()
offset = 0
header = '\x1b7\x1b[1;1H\x1b[2KUnicode flood: A漢🦀\u0378B\x1b8'.encode()
while time.monotonic() - start < 6:
    chunk = corpus[offset:offset+512]
    os.write(1, chunk.encode())
    offset += len(chunk)
    if offset == len(corpus): offset = 0
    os.write(1, header)
    time.sleep(0.004)
os.write(1, b'\x1b[r\x1b]2;unicode-done\x07')
"#;
