//! Native text and HTML clipboard behind the platform-neutral service (§13.1).
//!
//! Headless scenarios keep MemoryClipboard, so they never inspect a user's
//! desktop clipboard. The live shell creates an arboard handle per gesture:
//! this keeps the service Send without moving an OS clipboard handle across
//! the parser and window threads.
use puzzle_term::platform::Clipboard;
/// Binary transactions share the UI thread and return typed completion messages.
mod mime;
pub(crate) use mime::{complete, paste};

/// System clipboard provider for the live desktop window only.
pub(crate) struct ArboardClipboard;

/// The live provider keeps OS clipboard handles local to each transaction.
impl Clipboard for ArboardClipboard {
    /// Stores plain text after Session’s clipboard-write policies.
    fn write_text(&mut self, text: &str) -> Result<(), String> {
        let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
        clipboard.set_text(text).map_err(|error| error.to_string())
    }

    /// arboard publishes HTML and plain text in the same native transaction.
    fn write_html(&mut self, html: &str, text: &str) -> Result<(), String> {
        let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
        clipboard
            .set_html(html, Some(text))
            .map_err(|error| error.to_string())
    }

    /// Empty or non-text content produces no paste command.
    fn request_text(&mut self) -> Result<Option<String>, String> {
        let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
        match clipboard.get_text() {
            Ok(text) => Ok(Some(text)),
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
}
