//! Hand-authored K2 timeline with two PTY reads and two presented frames.
//!
//! The first frame contains only `he`; the second contains `hello`.
//! Ctrl+D follows under legacy flags and must write one C0 byte. The
//! fixture is literal JSON data rather than recorder output, so replay
//! cannot pass by serializing and parsing its own mistaken convention.
//! A second case changes only the expected transport byte and requires
//! the original file entry number in the failure report.

use super::{Replay, replay_file};
use serde_json::{Value as Json, json};
use std::{fs, path::PathBuf, time::SystemTime};

/// The fixture and replayed grids stay outside Cargo's crate directory.
struct TestOutput(PathBuf);

impl Drop for TestOutput {
    /// Remove generated grids and the private JSON recording after each test.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One explicit JSON Lines event with stable wire fields.
fn entry(number: usize, kind: &str, data: Json) -> Json {
    json!({"t": number, "n": number, "k": kind, "data": data})
}

/// Writes a small synthetic file with separate read and parse boundaries.
fn fixture(name: &str, wrong_byte: bool) -> Result<(PathBuf, TestOutput), String> {
    // Parallel test invocations need distinct output directories, and a
    // failed assertion must still release its generated files on drop.
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let output = TestOutput(
        std::env::temp_dir().join(format!("puzzle-k2-{name}-{}-{nonce}", std::process::id())),
    );
    fs::create_dir_all(&output.0).map_err(|error| error.to_string())?;
    let path = output.0.join("synthetic.pzrec");
    // The first snapshot occurs before the second read, proving that the
    // replayer does not feed future bytes into the earlier visible frame.
    let events = [
        entry(1, "header", json!({"grid": [8, 3], "dpr": 1})),
        entry(2, "pty-read", json!({"hex": "6865"})),
        entry(3, "parse", json!({"offered": 2, "total": 2})),
        entry(4, "snapshot", json!({"number": 0, "parsed": 2})),
        entry(5, "frame", json!({"number": 0, "parsed": 2})),
        entry(6, "pty-read", json!({"hex": "6c6c6f"})),
        entry(7, "parse", json!({"offered": 3, "total": 5})),
        entry(8, "snapshot", json!({"number": 1, "parsed": 5})),
        entry(9, "frame", json!({"number": 1, "parsed": 5})),
        entry(10, "input", json!({"kind": "modifiers", "bits": 4})),
        entry(11, "command", Json::Null),
        entry(
            12,
            "input",
            json!({"kind": "key", "logical": {"kind": "character", "value": "d"},
            "physical": "KeyD", "text": "\u{4}", "location": "Standard", "pressed": true, "repeat": false}),
        ),
        entry(
            13,
            "command",
            json!({"type": "input/key", "payload": {
            "key": "char:100", "modifiers": 4.0, "phase": "press", "shifted": null,
            "base": "d", "code": "KeyD", "text": null}}),
        ),
        entry(
            14,
            "pty-write",
            json!({"hex": if wrong_byte { "05" } else { "04" }}),
        ),
    ];
    let mut lines = String::new();
    for event in events {
        lines.push_str(&event.to_string());
        lines.push('\n');
    }
    fs::write(&path, lines).map_err(|error| error.to_string())?;
    Ok((path, output))
}

/// Both frozen grid states and the legacy Ctrl+D write match the fixture.
#[test]
fn synthetic_recording_replays_at_both_frames() -> Result<(), String> {
    let (path, output) = fixture("good", false)?;
    replay_file(&path, &output.0)?;
    let first = fs::read_to_string(output.0.join("replay-frame-000.grid.txt"))
        .map_err(|error| error.to_string())?;
    let second = fs::read_to_string(output.0.join("replay-frame-001.grid.txt"))
        .map_err(|error| error.to_string())?;
    assert_eq!(first.lines().next(), Some("he"));
    assert_eq!(second.lines().next(), Some("hello"));
    Ok(())
}

/// A one-byte adapter mismatch names the exact recorded PTY write entry.
#[test]
fn changed_adapter_byte_reports_entry() -> Result<(), String> {
    let (path, output) = fixture("bad", true)?;
    let result = replay_file(&path, &output.0);
    assert!(result.is_err_and(|error| error.contains("entry 14")
        && error.contains("04")
        && error.contains("05")));
    Ok(())
}

/// A recorded platform command ends replay only after its final parse entry.
#[test]
fn recorded_child_exit_replays_after_last_output() -> Result<(), String> {
    let mut replay = Replay::new(&json!({"grid": [8, 3], "dpr": 1}), false)?;
    let output = std::env::temp_dir();
    replay.entry(1, "pty-read", &json!({"hex": "646f6e65"}), &output, 1)?;
    replay.entry(2, "parse", &json!({"offered": 4, "total": 4}), &output, 2)?;
    replay.entry(
        3,
        "command",
        &json!({"type": "internal/childExited",
        "payload": {"code": 3.0, "signal": null, "runtimeMs": 500.0}}),
        &output,
        500,
    )?;
    let info = replay
        .session
        .exit_info()
        .ok_or("exit command was not replayed")?;
    if info.code != Some(3) || !replay.session.close_requested() {
        return Err("replay changed the final exit decision".into());
    }
    replay.finish()
}

/// M8 pointer commands are recorded after coalescing, without a RawInput key callback.
#[test]
fn direct_coalesced_pointer_replays_word_selection() -> Result<(), String> {
    let mut replay = Replay::new(&json!({"grid":[12, 3], "dpr":1}), false)?;
    replay.session.feed(b"alpha beta", replay.started);
    replay.entry(1, "command", &pointer_record(), &std::env::temp_dir(), 10)?;
    assert_eq!(replay.session.selection_text(), "alpha");
    replay.finish()
}

/// Direct pointer replay cannot bypass wire validation or the keyboard adapter witness.
#[test]
fn direct_pointer_rejects_malformed_payload_and_unwitnessed_key() -> Result<(), String> {
    let mut replay = Replay::new(&json!({"grid":[12, 3], "dpr":1}), false)?;
    let mut malformed = pointer_record();
    malformed["payload"]["origin"] = json!("platform");
    assert!(
        replay
            .entry(1, "command", &malformed, &std::env::temp_dir(), 10)
            .is_err()
    );
    let unwitnessed = json!({"type":"input/key", "payload":null});
    assert!(
        replay
            .entry(2, "command", &unwitnessed, &std::env::temp_dir(), 11)
            .is_err_and(|error| error.contains("preceding input"))
    );
    Ok(())
}

/// Hand-authored complete press map selects the first word with a real double-count.
fn pointer_record() -> Json {
    json!({"type":"input/pointer", "payload":{
        "event":{"kind":"press","held":true,"button":0.0,"column":0.0,"row":0.0,
        "pixelX":1.0,"pixelY":1.0,"modifiers":0.0,"clickCount":2.0},
        "x":1.0,"y":1.0,"cellWidth":10.0,"cellHeight":20.0
    }})
}
