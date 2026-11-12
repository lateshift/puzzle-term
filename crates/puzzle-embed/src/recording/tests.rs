//! Recorder unit checks for byte-exact control streams.

use super::{Recorder, hex, unhex};
use serde_json::{Value as Json, json};
use std::{fs, path::PathBuf, time::SystemTime};

/// A private test file is removed even if an assertion fails.
struct TestFile(PathBuf);

impl Drop for TestFile {
    /// Recording fixtures must not remain in the crate's working directory.
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// ESC, C0 and high bytes survive the JSON field's text encoding.
#[test]
fn hex_round_trip() {
    let bytes = [0, 4, 27, b'[', 127, 255];
    assert_eq!(unhex(&hex(&bytes)), Ok(bytes.to_vec()));
}

/// A partial or non-hex field reports bad external input rather than panic.
#[test]
fn malformed_hex_is_a_report() {
    assert!(unhex("0").is_err());
    assert!(unhex("zz").is_err());
}

/// Clipboard credentials never enter a recording, even across every possible read split.
#[test]
fn clipboard_stream_credentials_are_redacted() {
    let wire = b"before\x1b]5522;type=read:pw=c2VjcmV0:name=UGFzdGU=\x1b\\after\x1b]+05522;pw=c2VjcmV0\x07";
    for split in 0..=wire.len() {
        let mut filter = super::privacy::Redactor::default();
        let mut output = filter.filter(&wire[..split]);
        output.extend(filter.filter(&wire[split..]));
        assert_eq!(output.len(), wire.len());
        assert!(!output.windows(8).any(|part| part == b"c2VjcmV0"));
        assert!(output.starts_with(b"before\x1b]5522;"));
        assert!(output.windows(7).any(|part| part == b"\x1b\\after"));
        assert_eq!(output.last(), Some(&7));
    }
}

/// A requested file flushes complete ordered lines after each event.
#[test]
fn requested_path_writes_json_lines() -> Result<(), String> {
    // Cargo runs unit tests from the crate directory, so a relative target
    // path would create an untracked nested build tree there.
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let path = TestFile(std::env::temp_dir().join(format!(
        "puzzle-recording-{}-{nonce}.pzrec",
        std::process::id()
    )));
    let name = path.0.to_str().ok_or("temporary path is not UTF-8")?;
    let (recorder, _) = Recorder::open(Some(name))?;
    // A callback and its PTY bytes must remain separate entries even when
    // they share one millisecond timestamp on a fast native machine.
    recorder.emit("header", json!({"grid": [8, 3]}))?;
    recorder.bytes("pty-write", &[4])?;
    let text = fs::read_to_string(&path.0).map_err(|error| error.to_string())?;
    let values: Vec<Json> = text
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|error| error.to_string())?;
    assert_eq!(values.len(), 2);
    assert_eq!(values[0]["n"], 0);
    assert_eq!(values[1]["n"], 1);
    assert_eq!(values[1]["data"]["hex"], "04");
    Ok(())
}
