//! Optional JSON Lines evidence for the native event and PTY boundaries.
//!
//! Each line has monotonic milliseconds and an event kind (§13.4). A single
//! mutex orders records from the winit, drain and parser threads without
//! changing their normal scheduling. Recording is disabled unless requested;
//! the ordinary PTY path holds no file and does no byte formatting.
//!
//! Input and output may contain private text, so the default destination is
//! under ignored `target/recordings/`. A captured file must be inspected
//! before it can become a checked-in neutral regression fixture.
//!
//! The outer JSON object uses elapsed milliseconds `t`, an ordered index
//! `n`, an event kind `k` and event-specific `data`. Millisecond timestamps
//! can tie during one parser slice, so replay follows line order. Sorting
//! by time could move a key callback across a PTY read boundary.
//!
//! A PTY read and a VT parse are distinct under backpressure. The drain
//! records the first when the OS returns. The parser records the second
//! after a bounded slice. The UI records a frozen snapshot before building
//! GPU instances, then records presentation after the swapchain succeeds.
//! Parsing may continue during GPU work, so the presented frame can refer
//! to an earlier parsed offset than the latest parse entry.
//! Replay uses that offset to reproduce the state the user actually saw.
//!
//! The sink is synchronous only when recording is requested. A background
//! writer could reorder an input callback against a PTY write or lose the
//! final entries if a reproduction must be killed. Normal runs hold no
//! recorder and perform no byte formatting or diagnostic disk I/O.
//! Recording state includes independent redactors for both PTY directions.
//! Their framing survives arbitrary read boundaries and cannot mix incoming data
//! with outgoing replies. Filtering runs inside the recorder's ordered writer,
//! so concurrent diagnostic callers cannot reorder its framing state.
//! Only OSC 5522 private bodies change; byte offsets remain stable.

/// Protocol credentials are removed even from opt-in raw recordings.
mod privacy;

use crate::input::{RawIme, RawInput, RawKey, RawLogical};
use puzzle_term::messages::{Command, ToWire, Value};
use serde_json::{Value as Json, json};
use std::{
    fs::{self, File},
    io::{self, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

/// Shared, ordered sink for one live window's diagnostic run.
#[derive(Clone)]
pub struct Recorder {
    /// One lock preserves event order and whole JSON Lines records.
    inner: Arc<Mutex<Writer>>,
}

/// Decorates the blocking PTY reader without changing its read boundaries.
pub struct RecordingReader {
    /// Original PTY reader; the fixed pool still reads directly into buffers.
    inner: Box<dyn Read + Send>,
    /// Optional sink shared with UI events and successful PTY writes.
    recorder: Recorder,
}

impl RecordingReader {
    /// Installs the tap only when the user explicitly requests a recording.
    pub fn new(inner: Box<dyn Read + Send>, recorder: Recorder) -> Self {
        Self { inner, recorder }
    }
}

impl Read for RecordingReader {
    /// Records exactly one successful OS read as one pty-read entry.
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        // Capture the OS boundary before the pool hands the buffer to VT.
        // A later 16 KiB parser slice is a separate event in this format.
        let count = self.inner.read(bytes)?;
        if count > 0
            && let Err(error) = self.recorder.bytes("pty-read", &bytes[..count])
        {
            tracing::warn!(event = "desktop.record_failed", %error);
        }
        Ok(count)
    }
}

/// File and monotonic origin guarded by the recorder lock.
struct Writer {
    /// Each line is flushed so a killed reproduction remains useful.
    file: BufWriter<File>,
    /// Host timestamps are relative to this monotonic origin.
    start: Instant,
    /// Sequential index disambiguates equal millisecond timestamps.
    next: u64,
    /// Independent stream framing for split input and output packets.
    privacy: [privacy::Redactor; 2],
}

impl Recorder {
    /// Opens the user path or a timestamped file under ignored target data.
    pub fn open(path: Option<&str>) -> Result<(Self, PathBuf), String> {
        // A timestamp keeps default captures separate across reruns, while
        // an explicit path lets a user name a specific bug reproduction.
        let path = if let Some(path) = path {
            PathBuf::from(path)
        } else {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_millis();
            PathBuf::from(format!("target/recordings/{stamp}.pzrec"))
        };
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            // Only the diagnostic destination is created. Never write a
            // potentially private capture into checked-in fixture storage.
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let file = File::create(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let writer = Writer {
            file: BufWriter::new(file),
            start: Instant::now(),
            next: 0,
            privacy: Default::default(),
        };
        Ok((
            Self {
                inner: Arc::new(Mutex::new(writer)),
            },
            path,
        ))
    }

    /// Emits one complete event without interleaving another thread's JSON.
    pub fn emit(&self, kind: &str, data: Json) -> Result<(), String> {
        // The same lock assigns `n` and writes the line, so every reader
        // sees one total order even when three threads produce records.
        let mut writer = self.inner.lock().map_err(|error| error.to_string())?;
        writer.emit(kind, data)
    }

    /// Captures an OS callback before the adapter changes its meaning.
    pub fn input(&self, input: &RawInput) -> Result<(), String> {
        // Suppressed callbacks matter too: an IME duplicate must remain
        // visible in the trace even when it correctly emits no command.
        self.emit("input", raw_json(input))
    }

    /// Captures a typed adapter result in its stable bus wire shape.
    pub fn command(&self, command: Option<&Command>) -> Result<(), String> {
        // `null` proves that a callback made no command. It is especially
        // useful when an IME Commit follows the Ctrl modifier release.
        self.emit("command", command_json(command)?)
    }

    /// Keeps the exact OS read or successful PTY write as hexadecimal bytes.
    pub fn bytes(&self, kind: &str, bytes: &[u8]) -> Result<(), String> {
        // Redact complete OSC 5522 bodies across read boundaries. Keeping byte lengths
        // preserves parsed offsets; credential-bearing protocol traffic is not replayable.
        let mut writer = self.inner.lock().map_err(|error| error.to_string())?;
        let direction = usize::from(kind == "pty-write");
        let safe = writer.privacy[direction].filter(bytes);
        writer.emit(kind, json!({"hex": hex(&safe)}))
    }

    /// Records the parsed boundary captured with the frame snapshot.
    pub fn frame(&self, number: u64, parsed: u64) -> Result<(), String> {
        // The ordinal counts successful presentations. The parsed value
        // identifies the earlier snapshot, not the parser's latest offset.
        self.emit("frame", json!({"number": number, "parsed": parsed}))
    }
}

/// File emission assumes the caller already owns the recorder's ordering lock.
impl Writer {
    /// Writes and flushes one already privacy-filtered event atomically.
    fn emit(&mut self, kind: &str, data: Json) -> Result<(), String> {
        let record = json!({
            "t": self.start.elapsed().as_millis(),
            "n": self.next,
            "k": kind,
            "data": data,
        });
        self.next += 1;
        // JSON escaping prevents a typed newline from splitting a record.
        // Flushing a whole line retains evidence if the process is killed.
        serde_json::to_writer(&mut self.file, &record).map_err(|error| error.to_string())?;
        self.file
            .write_all(b"\n")
            .map_err(|error| error.to_string())?;
        self.file.flush().map_err(|error| error.to_string())
    }
}

/// Shares one wire projection between recording and adapter replay checks.
pub fn command_json(command: Option<&Command>) -> Result<Json, String> {
    // Rust Debug spelling is not a stable wire identity. The domain-owned
    // type string and checked payload remain comparable across builds.
    let Some(command) = command else {
        return Ok(Json::Null);
    };
    let value = command.to_value().map_err(|report| format!("{report:?}"))?;
    Ok(json!({"type": command.type_name(), "payload": value_json(&value)}))
}

/// Converts owned host input without relying on serde derives or winit.
fn raw_json(input: &RawInput) -> Json {
    // Retain even modified key text. macOS may supply a C0 text field for
    // Ctrl+D, which is distinct from the adapter's typed command result.
    match input {
        RawInput::Key(key) => {
            // The base-layer key is what Option-as-Alt must encode. It cannot
            // be recovered from a physical position across keyboard layouts,
            // so the callback records it alongside the composed identity.
            let logical = raw_logical_json(&key.logical);
            let base = raw_logical_json(&key.base);
            json!({"kind": "key", "logical": logical, "physical": key.physical,
                "base": base,
                "text": key.text, "location": key.location, "pressed": key.pressed,
                "repeat": key.repeat})
        }
        RawInput::Modifiers(bits) => json!({"kind": "modifiers", "bits": bits}),
        RawInput::Ime(ime) => match ime {
            RawIme::Preedit(text) => json!({"kind": "ime-preedit", "text": text}),
            RawIme::Commit(text) => json!({"kind": "ime-commit", "text": text}),
            RawIme::Enabled => json!({"kind": "ime-enabled"}),
            RawIme::Disabled => json!({"kind": "ime-disabled"}),
        },
        RawInput::Focus(focused) => json!({"kind": "focus", "focused": focused}),
    }
}

/// Serializes both modified and base key identities in the same wire shape.
fn raw_logical_json(key: &RawLogical) -> Json {
    match key {
        RawLogical::Named(name) => json!({"kind": "named", "value": name}),
        RawLogical::Character(text) => json!({"kind": "character", "value": text}),
        RawLogical::Dead => json!({"kind": "dead"}),
        RawLogical::Unidentified => json!({"kind": "unidentified"}),
    }
}

/// Maps the bounded bus tree to JSON, retaining byte arrays as hex objects.
fn value_json(value: &Value) -> Json {
    // The bus stays typed at runtime. Erasure occurs only for this optional
    // external file, where JSON has no distinct binary scalar.
    match value {
        Value::Null => Json::Null,
        Value::Bool(value) => json!(value),
        Value::Number(value) => json!(value),
        Value::String(value) => json!(value),
        Value::Bytes(value) => json!({"bytes": hex(value)}),
        Value::List(values) => Json::Array(values.iter().map(value_json).collect()),
        Value::Map(values) => Json::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), value_json(value)))
                .collect(),
        ),
    }
}

/// Hex is unambiguous for C0, ESC and read-boundary diagnostics.
pub fn hex(bytes: &[u8]) -> String {
    // Two digits per byte allow exact reconstruction of an incomplete CSI
    // sequence even when its next byte arrives in another PTY read.
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        // String formatting cannot fail because the destination owns space.
        let _ = write!(output, "{byte:02x}");
    }
    output
}

/// Reads one recorded byte field with validation for replay.
pub fn unhex(text: &str) -> Result<Vec<u8>, String> {
    // A trailing half byte means the recording is corrupt; silently
    // dropping it could turn a wrong key sequence into a false match.
    if !text.len().is_multiple_of(2) {
        return Err("hex byte field has an odd length".into());
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let part = std::str::from_utf8(pair).map_err(|error| error.to_string())?;
            u8::from_str_radix(part, 16).map_err(|error| error.to_string())
        })
        .collect()
}

/// Refuses to treat a directory as a recording file during replay.
pub fn read(path: &Path) -> Result<String, String> {
    // Name the private capture in an error rather than treating a missing
    // file as an empty, apparently successful reproduction.
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// Validates one externally recorded callback before adapter replay.
pub fn raw_from_json(data: &Json) -> Result<RawInput, String> {
    // This is external input. Every field is checked before constructing
    // the adapter type, including variant names and modifier narrowing.
    let kind = field_str(data, "kind")?;
    match kind {
        "key" => {
            let logical = data.get("logical").ok_or("key lacks logical identity")?;
            let logical = raw_logical_from_json(logical)?;
            // Older private recordings predate base-key capture. They still
            // replay their original adapter decisions for non-Option keys.
            let base = data
                .get("base")
                .map(raw_logical_from_json)
                .transpose()?
                .unwrap_or_else(|| logical.clone());
            Ok(RawInput::Key(RawKey {
                logical,
                base,
                physical: field_str(data, "physical")?.to_owned(),
                text: data.get("text").and_then(Json::as_str).map(str::to_owned),
                location: field_str(data, "location")?.to_owned(),
                pressed: field_bool(data, "pressed")?,
                repeat: field_bool(data, "repeat")?,
            }))
        }
        "modifiers" => Ok(RawInput::Modifiers(
            field_u64(data, "bits")?
                .try_into()
                .map_err(|_| "modifier bits exceed u8")?,
        )),
        "ime-preedit" => Ok(RawInput::Ime(RawIme::Preedit(
            field_str(data, "text")?.to_owned(),
        ))),
        "ime-commit" => Ok(RawInput::Ime(RawIme::Commit(
            field_str(data, "text")?.to_owned(),
        ))),
        "ime-enabled" => Ok(RawInput::Ime(RawIme::Enabled)),
        "ime-disabled" => Ok(RawInput::Ime(RawIme::Disabled)),
        "focus" => Ok(RawInput::Focus(field_bool(data, "focused")?)),
        other => Err(format!("unknown raw input kind: {other}")),
    }
}

/// Rejects an unknown key identity before replay reaches the adapter.
fn raw_logical_from_json(data: &Json) -> Result<RawLogical, String> {
    match field_str(data, "kind")? {
        "named" => Ok(RawLogical::Named(field_str(data, "value")?.to_owned())),
        "character" => Ok(RawLogical::Character(field_str(data, "value")?.to_owned())),
        "dead" => Ok(RawLogical::Dead),
        "unidentified" => Ok(RawLogical::Unidentified),
        other => Err(format!("unknown logical kind: {other}")),
    }
}

/// Reports a missing or mistyped JSON string at its field name.
pub fn field_str<'a>(data: &'a Json, name: &str) -> Result<&'a str, String> {
    data.get(name)
        .and_then(Json::as_str)
        .ok_or_else(|| format!("missing string {name}"))
}

/// Reports a missing or mistyped JSON boolean at its field name.
fn field_bool(data: &Json, name: &str) -> Result<bool, String> {
    data.get(name)
        .and_then(Json::as_bool)
        .ok_or_else(|| format!("missing bool {name}"))
}

/// Reports a missing or mistyped nonnegative integer at its field name.
pub fn field_u64(data: &Json, name: &str) -> Result<u64, String> {
    data.get(name)
        .and_then(Json::as_u64)
        .ok_or_else(|| format!("missing integer {name}"))
}

#[cfg(test)]
/// Round trips the text representation used by PTY read and write entries.
mod tests;
