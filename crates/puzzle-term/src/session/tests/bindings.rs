//! Shortcut execution through the real bus and transport, without a frame.
use super::*;
use crate::keybind::{Keybindings, Platform, Source};

/// Builds a table without platform shortcuts obscuring the input ordering.
fn bindings(lines: &[&str]) -> Keybindings {
    let mut bindings = Keybindings::default();
    bindings.apply(
        &lines.iter().map(|line| (*line).into()).collect::<Vec<_>>(),
        Source::User,
        Platform::Mac,
        false,
    );
    bindings
}

/// Records each policy visit; priority must span the borrowed built-in hook.
struct Visit(Writes, u8);
/// Downstream policies still receive public Key after leader replay.
impl Interceptor<Command, Event> for Visit {
    fn intercept(&mut self, command: &mut Command, _ctx: &mut HookCtx<'_, Command, Event>) -> Flow {
        if let Command::Key(key) = command {
            let byte = if let Key::Char(ch) = key.key {
                ch as u8
            } else {
                0
            };
            self.0.lock().expect("visits").push(vec![self.1, byte]);
        }
        Flow::Continue
    }
}

#[test]
/// Flushes cannot let a later key or sending action overtake the leader.
fn shortcut_batch_fifo_and_policy_priority() {
    let (mut session, writes, _) = fixture();
    session.set_keybindings(bindings(&["a>c=ignore", "b=sendText:B"]));
    let visits = Arc::new(Mutex::new(Vec::new()));
    for (priority, name, marker) in [(200, "before", 1), (0, "after", 2)] {
        session
            .bus_mut()
            .intercept(
                CommandKinds::at(0),
                priority,
                name,
                Visit(visits.clone(), marker),
            )
            .expect("hook");
    }
    for ch in ['a', 'x', 'b', 'y'] {
        session.submit(Command::Key(KeyEvent::press(Key::Char(ch))), Origin::User);
    }
    session.pump(Instant::now());
    assert_eq!(writes.lock().expect("writes").concat(), b"axBy");
    assert_eq!(
        *visits.lock().expect("visits"),
        [
            vec![1, b'a'],
            vec![1, b'x'],
            vec![1, b'b'],
            vec![1, b'y'],
            vec![2, b'a'],
            vec![2, b'x'],
            vec![2, b'y']
        ]
    );
}

#[test]
/// Timeout/reload must preserve held press-release order under kitty flags.
fn shortcut_timeout_reload_and_core_actions() {
    let (mut session, writes, _) = fixture();
    let now = Instant::now();
    let mut table = bindings(&["a>c=ignore"]);
    table.timeout_ms = 10;
    session.set_keybindings(table);
    session.feed(b"\x1b[>3u", now);
    let press = KeyEvent::press(Key::Char('a'));
    let mut release = press.clone();
    release.phase = crate::vt::input::KeyPhase::Release;
    session.submit(Command::Key(press.clone()), Origin::Api);
    session.submit(Command::Key(release.clone()), Origin::Api);
    session.pump(now);
    assert!(writes.lock().expect("writes").is_empty());
    session.pump(now + Duration::from_millis(11));
    assert_eq!(
        writes.lock().expect("writes").concat(),
        [
            crate::vt::input::encode_key(&press, InputModes::from_terminal(session.terminal())),
            crate::vt::input::encode_key(&release, InputModes::from_terminal(session.terminal()))
        ]
        .concat()
    );
    session.set_keybindings(bindings(&["x=clearScreen", "y=clearScrollback", "z=reset"]));
    session.feed(b"old\r\n\x1b[31mprompt$ ", now);
    session.submit(Command::Key(KeyEvent::press(Key::Char('x'))), Origin::User);
    session.pump(now);
    let view = session.buffer();
    assert_eq!(
        view.line(view.viewport().expect("viewport").first)
            .expect("prompt")
            .cells()
            .iter()
            .take(8)
            .filter_map(|cell| char::from_u32(cell.ch))
            .collect::<String>(),
        "prompt$ "
    );
    assert_eq!(session.terminal().cursor.row, 0);
    session.submit(Command::Key(KeyEvent::press(Key::Char('z'))), Origin::User);
    session.pump(now);
    assert_eq!(InputModes::from_terminal(session.terminal()).kitty_flags, 0);
}
