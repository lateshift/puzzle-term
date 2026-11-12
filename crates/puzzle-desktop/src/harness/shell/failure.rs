//! Exact transcript capture belongs to the child whose pixels failed.
use super::*;
use std::path::Path;

/// Captures this child while its buffers still belong to the failing scenario.
impl ShellProbe {
    /// Writes raw output and every input/reply boundary before this child drops.
    pub(in crate::harness) fn save_failure(&self, output: &Path) -> Result<(), String> {
        // A real PTY transcript locates incomplete child redraws separately from OCR.
        fs::write(output.join("pty-output.bin"), &self.raw).map_err(|error| error.to_string())?;
        // The observer lock freezes write boundaries; it never guards a Session.
        let writes = self.writes.lock().map_err(|error| error.to_string())?;
        // Key byte offsets identify the precise chord, flags and output boundary.
        // JSON arrays preserve embedded NUL and escape bytes without lossy UTF-8.
        let transcript = serde_json::json!({
            "writes": *writes,
            "key_start": self.key_start,
            "key_end": self.key_end,
            "key_flags": self.key_flags,
            "key_output_start": self.key_output_start,
        });
        // Keeping write boundaries proves one chord did not become two terminal writes.
        fs::write(output.join("pty-input.json"), transcript.to_string())
            .map_err(|error| error.to_string())
    }
}
