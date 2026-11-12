//! Host paste and terminal-origin clipboard policy checks (§6.8, §16).
use super::*;
use crate::platform::{Clipboard, MemoryClipboard};

/// Host paste is one typed command, with mode 2004 sampled at dispatch.
#[test]
fn memory_clipboard_paste_obeys_bracketed_mode() {
    let (mut session, writes, _) = fixture();
    session.services.clipboard = Box::new(MemoryClipboard {
        text: Some("a\nb".into()),
    });
    // The same clipboard text takes different wire forms after a program
    // enables bracketed paste; it is never sent as synthetic key presses.
    session.paste_from_clipboard(Instant::now()).expect("paste");
    session.feed(b"\x1b[?2004h", Instant::now());
    session
        .paste_from_clipboard(Instant::now())
        .expect("bracketed paste");
    assert_eq!(
        *writes.lock().expect("writes"),
        [b"a\rb".to_vec(), b"\x1b[200~a\rb\x1b[201~".to_vec()]
    );
}

/// OSC 52 may write under policy, while a read query remains disabled.
#[test]
fn osc52_write_reaches_clipboard_and_read_is_denied() {
    /// Shared text shows whether the host clipboard method was reached.
    struct ProbeClipboard(Arc<Mutex<Option<String>>>);
    impl Clipboard for ProbeClipboard {
        /// Stores decoded, policy-approved OSC text for the assertion.
        fn write_text(&mut self, text: &str) -> Result<(), String> {
            *self.0.lock().expect("clipboard") = Some(text.to_owned());
            Ok(())
        }
        /// OSC 52 queries must not call the host read capability.
        fn request_text(&mut self) -> Result<Option<String>, String> {
            panic!("OSC 52 read reached host clipboard")
        }
    }
    let (mut session, writes, _) = fixture();
    let text = Arc::new(Mutex::new(None));
    session.services.clipboard = Box::new(ProbeClipboard(text.clone()));
    session.feed(b"\x1b]52;c;aGVsbG8=\x07", Instant::now());
    assert_eq!(*text.lock().expect("clipboard"), Some("hello".to_owned()));
    // A query must neither call the clipboard's read method nor produce
    // reply bytes that leak host clipboard text into the child process.
    session.feed(b"\x1b]52;c;?\x07", Instant::now());
    assert!(writes.lock().expect("writes").is_empty());
}
