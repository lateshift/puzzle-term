//! Output-only .vtrec writer: delay_ms, byte length, raw bytes (§12.5).
//!
//! Recording is opt-in and may contain private shell output. A separate file
//! per accepted session avoids interleaving unrelated PTYs into one stream.
//! create_new refuses to overwrite an earlier recording, including symlinks.
use std::{fs::File, io::Write, path::Path, time::Instant};

/// A writer belongs to the PTY reader, preserving read ordering and timing.
pub(crate) struct Recorder {
    /// Private output destination created before a shell starts.
    file: File,
    /// Previous recorded read, not the previous socket send.
    last: Instant,
}

/// Records only bounded PTY chunks, so u32 lengths never truncate data.
impl Recorder {
    /// Opens the base path for the first session, then numbered siblings.
    pub(crate) fn open(base: &Path, session: u64) -> Result<Self, String> {
        // A single requested path cannot mix simultaneous sessions: suffixes
        // preserve each child's independent byte stream and timing origin.
        let path = if session == 1 {
            base.to_path_buf()
        } else {
            base.with_extension(format!("{session}.vtrec"))
        };
        let file = File::options()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| format!("record {}: {error}", path.display()))?;
        Ok(Self {
            file,
            last: Instant::now(),
        })
    }

    /// Delays saturate after 49 days; bytes never get silently truncated.
    pub(crate) fn bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        let now = Instant::now();
        let delay = now
            .duration_since(self.last)
            .as_millis()
            .min(u128::from(u32::MAX)) as u32;
        self.last = now;
        // Three fields match the existing replay format without JSON or
        // a timestamp parser. A failed write aborts this recording session.
        self.file
            .write_all(&delay.to_le_bytes())
            .and_then(|()| self.file.write_all(&(bytes.len() as u32).to_le_bytes()))
            .and_then(|()| self.file.write_all(bytes))
            .and_then(|()| self.file.flush())
            .map_err(|error| error.to_string())
    }
}
