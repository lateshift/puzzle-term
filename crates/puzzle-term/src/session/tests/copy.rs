//! Clipboard formats, plain alternatives and interception at the public Session seam.
use super::*;
use crate::{
    keybind::{Keybindings, Platform, Source},
    platform::Clipboard,
    selection::{CopyTarget, SelectionCommand},
    vt::view::Format,
};

/// Each accepted write stores text plus optional rich data for independent assertions.
type Copies = Arc<Mutex<Vec<(String, Option<String>)>>>;
/// A fake clipboard verifies one transaction without touching the user's clipboard.
struct Probe(Copies);
/// Records both formats instead of allowing rich markup to masquerade as plain text.
impl Clipboard for Probe {
    /// Records the plain representation after policy approval.
    fn write_text(&mut self, text: &str) -> Result<(), String> {
        self.0.lock().expect("copies").push((text.into(), None));
        Ok(())
    }
    /// Records one rich transaction together with its mandatory plain alternative.
    fn write_html(&mut self, html: &str, text: &str) -> Result<(), String> {
        self.0
            .lock()
            .expect("copies")
            .push((text.into(), Some(html.into())));
        Ok(())
    }
    /// This witness never reads the operating-system clipboard.
    fn request_text(&mut self) -> Result<Option<String>, String> {
        Ok(None)
    }
}
/// Reuses parser/service setup while retaining clipboard output outside Session.
fn copy_fixture() -> (TestHost, Copies, Writes) {
    let (mut session, writes, _) = fixture();
    let copies = Arc::new(Mutex::new(Vec::new()));
    session.services.clipboard = Box::new(Probe(copies.clone()));
    (session, copies, writes)
}
/// Known SGR and markup must survive in appropriate representations only.
#[test]
fn copy_formats_and_plain_alternative() {
    let (mut session, copies, writes) = copy_fixture();
    let now = Instant::now();
    session.feed(b"\x1b[4;9;38;2;200;100;50;48;2;20;40;60m<&\x1b[7;2mX", now);
    let id = session.buffer().cursor().expect("row").line;
    session.submit(
        Command::Select(SelectionCommand::Range {
            kind: crate::vt::selection::SelectionKind::Simple,
            start: crate::vt::selection::SelectPoint {
                line: id,
                column: 0,
            },
            end: crate::vt::selection::SelectPoint {
                line: id,
                column: 2,
            },
        }),
        Origin::Api,
    );
    session.pump(now);
    for format in [Format::PlainText, Format::Vt, Format::Html] {
        session.submit(Command::Copy(CopyTarget::Selection(format)), Origin::User);
        session.pump(now);
    }
    let actual = copies.lock().expect("copies");
    assert_eq!(actual.len(), 3);
    assert_eq!(actual[0], ("<&X".into(), None));
    assert!(actual[1].0.contains(";9;4:1"));
    assert!(actual[1].0.ends_with("\x1b[0m"));
    assert_eq!(actual[2].0, "<&X");
    let html = actual[2].1.as_ref().expect("rich alternative");
    assert!(html.contains("text-decoration:underline line-through"));
    assert!(html.contains("color:#6e4637;background-color:#c86432"));
    assert_eq!(html.matches("<span ").count(), 2);
    assert!(html.contains("&lt;&amp;"));
    assert!(writes.lock().expect("PTY").is_empty());
}
/// Empty selection preserves Ctrl+C; populated selection copies without interrupt bytes.
#[test]
fn performable_copy_passes_interrupt_without_selection() {
    for platform in [Platform::Windows, Platform::Linux] {
        let (mut session, copies, writes) = copy_fixture();
        let mut bindings = Keybindings::default();
        assert!(
            bindings
                .apply(
                    &["performable:ctrl+c=copy".into()],
                    Source::User,
                    platform,
                    false
                )
                .is_empty()
        );
        session.set_keybindings(bindings);
        let mut key = KeyEvent::press(Key::Char('c'));
        key.modifiers = crate::vt::input::Modifiers(4);
        session.submit(Command::Key(key.clone()), Origin::User);
        session.pump(Instant::now());
        assert_eq!(writes.lock().expect("PTY").concat(), b"\x03");
        session.feed(b"selected", Instant::now());
        session.submit(Command::Select(SelectionCommand::All), Origin::User);
        session.pump(Instant::now());
        session.submit(Command::Key(key), Origin::User);
        session.pump(Instant::now());
        assert_eq!(writes.lock().expect("PTY").concat(), b"\x03");
        assert!(copies.lock().expect("copies")[0].0.starts_with("selected"));
    }
}
/// All write representations share the existing policy bit and can be vetoed.
#[test]
fn rich_copy_policy_and_latest_finished_output() {
    let (mut session, copies, _) = copy_fixture();
    let now = Instant::now();
    session.feed(
        b"\x1b]133;C\x07first\r\n\x1b]133;D;0\x07\x1b]133;A\x07prompt\x1b]133;C\x07unfinished",
        now,
    );
    session.submit(Command::Copy(CopyTarget::CommandOutput), Origin::User);
    session.pump(now);
    assert_eq!(copies.lock().expect("copies")[0].0, "first\n");
    /// This policy sees the shared write kind regardless of its representation.
    struct Deny;
    /// ClipboardWrite veto applies to both payload representations.
    impl Interceptor<Command, Event> for Deny {
        /// Deny before any fake-service write can occur.
        fn intercept(
            &mut self,
            _command: &mut Command,
            _ctx: &mut HookCtx<'_, Command, Event>,
        ) -> Flow {
            Flow::Stop("test clipboard denied")
        }
    }
    session
        .bus_mut()
        .intercept(CommandKinds::at(18), 0, "deny-copy", Deny)
        .expect("policy");
    session.submit(Command::Select(SelectionCommand::All), Origin::Api);
    session.pump(now);
    for format in [Format::PlainText, Format::Vt, Format::Html] {
        session.submit(Command::Copy(CopyTarget::Selection(format)), Origin::User);
        session.pump(now);
    }
    assert_eq!(copies.lock().expect("copies").len(), 1);
}

/// A completed command with no output supersedes the earlier nonempty command.
#[test]
fn copy_latest_empty_command_output() {
    let (mut session, copies, _) = copy_fixture();
    let now = Instant::now();
    session.feed(
        b"\x1b]133;C\x07first\x1b]133;D;0\x07\x1b]133;A\x07prompt\x1b]133;C\x07\x1b]133;D;0\x07",
        now,
    );
    session.submit(Command::Copy(CopyTarget::CommandOutput), Origin::User);
    session.pump(now);
    assert_eq!(copies.lock().expect("copies")[0].0, "");
}
