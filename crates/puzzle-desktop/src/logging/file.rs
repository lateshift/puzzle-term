//! Line-flushed native trace file with one previous-run rotation (§5.6).
//!
//! The tracing formatter may call Write several times for one event. A
//! per-event writer buffers those pieces until a newline, then writes and
//! flushes the complete record under one mutex. This prevents concurrent
//! parser and UI events from interleaving bytes in the same log line.
//! Rotation happens between records, so a timestamp and its fields never
//! split across PATH and PATH.1. The file handle is closed before rename,
//! which also permits rotation on platforms that forbid renaming open files.
//!
//! SystemTime in tracing-subscriber formats UTC with six fractional digits.
//! It needs no extra time crate and does not depend on the user's locale.
//!
//! Startup rotation and size rotation use the same rename rule. Each path
//! has at most a current file and one `.1` predecessor. A second startup
//! replaces that predecessor before opening an empty current file; an
//! in-run size rollover performs the same operation between complete
//! records. The mutex also protects the file handle while it is closed
//! and reopened, so a parser event cannot write into the old run.
//!
//! Logging failures must not make the terminal unable to start. Opening
//! happens before subscriber installation so that failure can be reported
//! through the still-available console layer. An I/O error after startup
//! is returned through Write to tracing-subscriber; the writer does not
//! call tracing recursively while holding its own log mutex.

use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};
use tracing_subscriber::fmt::MakeWriter;

/// Maximum bytes in the current run before moving it to PATH.1.
const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Shared rotating file state used by both parser and window events.
pub(super) struct FileSink {
    /// One lock covers whole records and the close-rename-open transition.
    state: Mutex<RotatingFile>,
}

impl FileSink {
    /// Moves an existing run aside, creates parent directories, and opens new.
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        Self::open_limit(path, MAX_BYTES)
    }

    /// The test limit exercises rotation without writing 64 MiB of logs.
    fn open_limit(path: &Path, limit: u64) -> io::Result<Self> {
        // An absent nested directory is ordinary when a user asks for a
        // fresh diagnostic path such as target/recordings/session.log.
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let previous = previous_path(path);
        if path.exists() {
            // The previous-run file reflects the complete prior process,
            // not an append to whatever was left by a crash or old launch.
            replace_previous(path, &previous)?;
        }
        let file = new_file(path)?;
        Ok(Self {
            state: Mutex::new(RotatingFile {
                path: path.to_owned(),
                previous,
                file: Some(file),
                written: 0,
                limit,
            }),
        })
    }
}

/// A writer holds the mutex for one formatted event, then releases it.
pub(super) struct EventWriter<'a> {
    /// The guard prevents two lines from interleaving during rotation.
    state: MutexGuard<'a, RotatingFile>,
    /// The formatter may write a record in multiple pieces.
    pending: Vec<u8>,
}

impl<'a> MakeWriter<'a> for FileSink {
    /// One guard and buffer belong to one tracing event.
    type Writer = EventWriter<'a>;

    /// Recovers a poisoned lock so diagnostics remain available after panic.
    fn make_writer(&'a self) -> Self::Writer {
        // A poisoned lock follows a prior panic, but diagnostics should
        // still be writable while the process reports that failure.
        EventWriter {
            state: self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            pending: Vec::new(),
        }
    }
}

impl Write for EventWriter<'_> {
    /// Flushes complete newline-terminated records before returning.
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        // fmt may send the timestamp, level, fields and newline in several
        // calls. Wait for newline before considering this a durable record.
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|&byte| byte == b'\n') {
            // Drain one whole record so another thread cannot divide it.
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            self.state.write_line(&line)?;
        }
        Ok(bytes.len())
    }

    /// Makes an unfinished record durable when the formatter requests it.
    fn flush(&mut self) -> io::Result<()> {
        // This path is only for a formatter that ended without newline;
        // normal records were already flushed in write().
        if !self.pending.is_empty() {
            self.state.write_line(&self.pending)?;
            self.pending.clear();
        }
        Ok(())
    }
}

impl Drop for EventWriter<'_> {
    /// A formatter without a newline must not lose its final fields.
    fn drop(&mut self) {
        // A destructor cannot return I/O errors. The normal newline path
        // already returned them while formatting the trace event.
        let _ = self.flush();
    }
}

/// Current file and its single retained predecessor.
struct RotatingFile {
    /// Log file requested by environment, CLI, or future settings.
    path: PathBuf,
    /// Same path with `.1` appended, rather than changing its extension.
    previous: PathBuf,
    /// Taken before rename so Windows can rotate an open destination.
    file: Option<File>,
    /// Bytes already committed to this run's active file.
    written: u64,
    /// Production uses 64 MiB; tests use a smaller deterministic bound.
    limit: u64,
}

impl RotatingFile {
    /// Writes and flushes exactly one record, rotating before it crosses cap.
    fn write_line(&mut self, line: &[u8]) -> io::Result<()> {
        let length = u64::try_from(line.len()).unwrap_or(u64::MAX);
        // Rotation occurs before the next complete line crosses the cap;
        // no timestamp can end up separated from its event fields.
        if self.written > 0 && self.written.saturating_add(length) > self.limit {
            self.rotate()?;
        }
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("log file closed"))?;
        file.write_all(line)?;
        // A complete record survives a process crash after this flush;
        // only the formatter's current, incomplete line can be lost.
        file.flush()?;
        self.written = self.written.saturating_add(length);
        Ok(())
    }

    /// Keeps only the most recent old run before reopening the active path.
    fn rotate(&mut self) -> io::Result<()> {
        // Drop the active handle before rename for Windows and Unix parity.
        self.file.take();
        replace_previous(&self.path, &self.previous)?;
        self.file = Some(new_file(&self.path)?);
        self.written = 0;
        Ok(())
    }
}

/// Appends `.1` to the entire filename so `trace.log` becomes `trace.log.1`.
fn previous_path(path: &Path) -> PathBuf {
    // Appending to the whole OS string also preserves non-UTF-8 native
    // filenames, which a formatted String conversion would corrupt.
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(".1");
    PathBuf::from(name)
}

/// Replaces the one retained previous run with the current file.
fn replace_previous(path: &Path, previous: &Path) -> io::Result<()> {
    // Windows rename cannot replace an existing destination. Removing only
    // the one prior log bounds disk retention to two runs.
    if previous.exists() {
        fs::remove_file(previous)?;
    }
    fs::rename(path, previous)
}

/// Opens a fresh active run without appending stale records.
fn new_file(path: &Path) -> io::Result<File> {
    // `truncate` makes each startup or rollover a new run; append mode
    // would defeat the 64 MiB bound after an interrupted process.
    OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
}

#[cfg(test)]
mod tests;
