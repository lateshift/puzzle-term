//! Logical byte accounting used unchanged by the preflight and copy passes.
//! Pending ASCII spaces avoid a per-row buffer while preserving trim policy.
//! Empty/hard rows are represented by the walker, not guessed from output bytes.
//!
//! A long run of padding may exceed the payload cap yet disappear under trim.
//! Count it in a scalar counter first, not in a temporary row allocation.
//! If later content or a soft wrap makes it significant, admit the entire run
//! before materializing it. This preserves the existing trailing-space policy
//! without rejecting harmless trimmed padding or exporting a shortened reply.
use crate::osc::SourceReadError;

/// No text allocation exists during the counting pass.
#[derive(Default)]
pub(super) struct Sink {
    /// Present only after the whole output fits its consumer allowance.
    text: Option<String>,
    /// Bytes committed in this pass, including hard-line separators.
    length: usize,
    /// Trailing spaces are deferred until another visible byte or a soft wrap.
    spaces: usize,
    /// Expected final length from preflight; a divergent copy is refused whole.
    expected: usize,
}

impl Sink {
    /// Trimming is limited to ASCII spaces, matching the contents API.
    pub(super) fn emit(&mut self, text: &str, limit: usize) -> Result<(), SourceReadError> {
        // Only ASCII spaces are padding in contents extraction; tabs, NBSP and
        // Unicode whitespace remain exact source bytes, including inside clusters.
        let content = text.trim_end_matches(' ');
        // A non-space suffix makes all preceding deferred spaces significant.
        // Spaces inside this part are already preserved by append unchanged.
        if !content.is_empty() {
            self.flush_spaces(limit)?;
            self.append(content, limit)?;
        }
        // Do not charge trimmed padding as retained text. Its counter itself is
        // inline metadata, and checked arithmetic refuses rather than wrapping.
        self.spaces = self
            .spaces
            .checked_add(text.len() - content.len())
            .ok_or(SourceReadError::Rejected)?;
        Ok(())
    }

    /// A hard row trims pending padding; wrapped rows retain it as logical text.
    pub(super) fn end_row(&mut self, trim: bool, limit: usize) -> Result<(), SourceReadError> {
        // A hard logical boundary is the only place trimming discards padding.
        // A wrapped line flushes instead because its spaces precede more text.
        if trim {
            self.spaces = 0;
            Ok(())
        } else {
            self.flush_spaces(limit)
        }
    }

    /// A separator is real text, unlike a lower rectangular reservation row.
    pub(super) fn newline(&mut self, limit: usize) -> Result<(), SourceReadError> {
        self.append("\n", limit)
    }

    /// Large runs are counted before any insertion, never materialized separately.
    fn flush_spaces(&mut self, limit: usize) -> Result<(), SourceReadError> {
        // Refuse the complete run before writing even its first space. The
        // second pass cannot grow past the capacity admitted by its first pass.
        let next = self
            .length
            .checked_add(self.spaces)
            .ok_or(SourceReadError::Rejected)?;
        if next > limit {
            return Err(SourceReadError::Rejected);
        }
        if let Some(text) = &mut self.text {
            if next > text.capacity() {
                return Err(SourceReadError::Rejected);
            }
            // Iterator insertion writes directly into the exact-capacity output;
            // no intermediate repeated-space String or Vec is constructed.
            text.extend(std::iter::repeat_n(' ', self.spaces));
        }
        self.length = next;
        self.spaces = 0;
        Ok(())
    }

    /// Count/copy share the exact same overflow and remaining-budget checks.
    fn append(&mut self, part: &str, limit: usize) -> Result<(), SourceReadError> {
        let next = self
            .length
            .checked_add(part.len())
            .ok_or(SourceReadError::Rejected)?;
        if next > limit {
            return Err(SourceReadError::Rejected);
        }
        if let Some(text) = &mut self.text {
            if next > text.capacity() {
                return Err(SourceReadError::Rejected);
            }
            text.push_str(part);
        }
        self.length = next;
        Ok(())
    }

    /// The only allocation happens after complete bounded preflight.
    pub(super) fn start_copy(&mut self, limit: usize) -> Result<(), SourceReadError> {
        // Preflight has seen every selected row. This allocation is proportional
        // to admitted logical UTF-8 bytes, never the grid or scrollback extent.
        self.expected = self.length;
        let mut text = String::new();
        text.try_reserve_exact(self.expected)
            .map_err(|_| SourceReadError::Rejected)?;
        // Account the actual capacity, even if an allocator supplies extra space.
        if text.capacity() > limit {
            return Err(SourceReadError::Rejected);
        }
        self.text = Some(text);
        // The second pass has independent byte and trailing-space counters.
        // Reusing the preflight count here would incorrectly double the output.
        self.length = 0;
        self.spaces = 0;
        Ok(())
    }

    /// No count/copy discrepancy can turn a truncated value into a host payload.
    pub(super) fn finish_copy(&self) -> Result<(), SourceReadError> {
        if self.length == self.expected {
            Ok(())
        } else {
            Err(SourceReadError::Changed)
        }
    }

    /// Actual heap bytes are part of the consumer's combined retained ledger.
    pub(super) fn capacity(&self) -> usize {
        self.text.as_ref().map_or(0, String::capacity)
    }

    /// Moving the result does not duplicate its contents or allocation.
    pub(super) fn take(&mut self) -> Option<String> {
        // None on a repeated handoff prevents a second empty drag payload.
        self.text.take()
    }

    /// Failure retains no export, and take_text remains disabled permanently.
    pub(super) fn discard(&mut self) {
        self.text = None;
    }
}
