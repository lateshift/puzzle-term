//! Deterministic terminal-byte corpora for the §15.2 parse budgets.
//!
//! Every fixture repeats complete control/text records until at least
//! 64 KiB. The native examples repeat these same recorded bytes to about
//! 8 MiB per timed sample, keeping generation outside measurement. Inputs
//! contain no machine names, paths or user data. The cursor corpus models
//! a full-screen process that repaints rows with CUP/EL and short fields;
//! the SGR corpus changes indexed and RGB rendition within each row.
use std::{fs, path::Path};

/// Directory of regenerated, checked-in benchmark source streams.
const CORPUS_DIR: &str = "assets/fixtures/corpus";
/// Minimum source size; timed probes repeat rather than truncate records.
const SOURCE_BYTES: usize = 64 * 1024;

/// Writes every stream from fixed, whole records for reproducible timing.
pub(super) fn write_all() -> Result<(), String> {
    fs::create_dir_all(CORPUS_DIR).map_err(|error| error.to_string())?;
    for (name, bytes) in [
        ("plain-ascii", plain_ascii()),
        ("sgr-heavy", sgr_heavy()),
        ("cursor-heavy", cursor_heavy()),
        ("unicode-mix", unicode_mix()),
        ("flood", flood()),
    ] {
        let path = Path::new(CORPUS_DIR).join(format!("{name}.vt"));
        fs::write(path, bytes).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Repeats one complete terminal record without cutting an escape sequence.
fn repeat(record: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(SOURCE_BYTES + record.len());
    while bytes.len() < SOURCE_BYTES {
        bytes.extend_from_slice(record);
    }
    bytes
}

/// Feeds printable 78-column rows with CR/LF at the screen margin.
fn plain_ascii() -> Vec<u8> {
    let mut row = [b'A'; 80];
    row[78] = b'\r';
    row[79] = b'\n';
    repeat(&row)
}

/// Alternates common SGR color and style forms with actual printable text.
fn sgr_heavy() -> Vec<u8> {
    // Both palette and RGB paths dispatch through the parser's SGR logic;
    // the text keeps grid writes in the timed operation.
    repeat(b"\x1b[1;38;5;33mPID 1024  cpu 12.5%\x1b[0m  \x1b[48;2;28;31;42mstatus ok\x1b[0m\r\n")
}

/// Repaints a 24-row dashboard with CUP, erase and short colored fields.
fn cursor_heavy() -> Vec<u8> {
    let mut frame = Vec::new();
    for row in 1..=24 {
        // CUP and EL exercise parser state and row edits; field text is
        // comparable to htop's repeated status/process-list updates.
        frame.extend_from_slice(
            format!("\x1b[{row};1H\x1b[2K\x1b[3{digit}m", digit = row % 8).as_bytes(),
        );
        frame.extend_from_slice(b"  PID  CPU% MEM%  command  task-running");
    }
    repeat(&frame)
}

/// Mixes narrow, wide and combining Unicode with ANSI grid movement.
fn unicode_mix() -> Vec<u8> {
    // This is a diagnostic corpus without an M5 native rate floor. It
    // catches width/cluster performance separately from ASCII shortcuts.
    repeat("Latin e\u{0301} | 界面 | λπ | 👩‍💻 | café\r\n".as_bytes())
}

/// Repaints every cell of an 80×40 truecolor half-block fire frame.
fn flood() -> Vec<u8> {
    let mut stream = Vec::new();
    for frame in 0..4_u32 {
        // Home and row-addressing keep the entire workload on one screen,
        // mirroring an animation rather than scrolling a command log.
        stream.extend_from_slice(b"\x1b[H");
        for row in 0..40_u32 {
            stream.extend_from_slice(format!("\x1b[{};1H", row + 1).as_bytes());
            for column in 0..80_u32 {
                // Each cell changes both half-block colors. Four phases
                // make a repeated source visibly animate instead of redraw
                // the same pixels, while remaining deterministic on disk.
                let heat = (row * 11 + column * 3 + frame * 37) % 256;
                let upper = format!("\x1b[38;2;{heat};{};0m", heat / 3);
                let lower = format!("\x1b[48;2;{};{};0m▀", heat / 2, heat / 5);
                stream.extend_from_slice(upper.as_bytes());
                stream.extend_from_slice(lower.as_bytes());
            }
        }
    }
    stream
}
