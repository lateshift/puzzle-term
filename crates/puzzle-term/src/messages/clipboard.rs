//! Typed clipboard representations behind one write-policy boundary (§14.9).
//!
//! Text-only OSC 52 and copy retain their existing string wire form. Rich copy
//! carries HTML and equivalent text together, so a veto covers both formats.
//! Platform services publish the pair in one transaction, avoiding separate
//! writes that could overwrite an alternative. No MIME names leak into core.

/// One accepted clipboard transaction, owned until the host consumes it.
#[derive(Debug)]
pub enum ClipboardContent {
    /// Correlated MIME write crosses the same interceptor mask as OSC 52.
    Mime {
        /// Internal correlation; zero denotes an ordinary external write.
        token: u64,
        /// All alternatives form one atomic host operation.
        items: Vec<crate::clipboard::MimeData>,
    },
    /// Plain Unicode or VT escape-sequence text in the legacy representation.
    Text(String),
    /// Styled copy must also remain useful to a terminal or a plain editor.
    Html {
        /// Escaped inline-styled fragment describing the selected cells.
        html: String,
        /// Unicode extraction of exactly the same selected interval.
        text: String,
    },
}

/// Ordinary callers keep a direct conversion from their existing text buffer.
impl From<String> for ClipboardContent {
    /// Takes ownership without copying text or assigning a rich MIME type.
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}
