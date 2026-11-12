//! Real interactive shells produce semantic zones through the native PTY harness.
//! Each fixture sources twice after a framework-style prompt and verifies exact
//! command output, failure status, preserved hooks and a URI-encoded directory.
use super::*;
use puzzle_term::vt::zones::ZoneKind;
use std::path::PathBuf;

/// Kill/reap even when a protocol assertion fails, so check cannot leak shells.
struct ChildGuard(Box<dyn Child + Send + Sync>);
impl Drop for ChildGuard {
    /// Reap on assertion failure as well as on a successful protocol cycle.
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
/// One Session answers real terminal queries while the shell starts and edits input.
struct Peer {
    /// Real parser and ordered query replies, driven on this test owner.
    session: Session,
    /// Blocking reader hands off bytes without sharing the parser.
    chunks: mpsc::Receiver<Vec<u8>>,
    /// Raw OSC and prompt bytes retained for failures and review.
    transcript: Vec<u8>,
    /// Unique workspace evidence directory; never a crate-local target.
    directory: PathBuf,
    /// Own the child until all output has been compared.
    _child: ChildGuard,
}
impl Peer {
    /// Wait for the next semantic input boundary; speed is not asserted in debug.
    fn prompt(&mut self, count: usize) {
        // Five minutes is a deadlock guard, not a shell startup performance budget.
        let deadline = Instant::now() + crate::test_support::HANG_GUARD;
        while self
            .session
            .buffer()
            .zones()
            .filter(|z| z.kind == ZoneKind::Input)
            .count()
            < count
        {
            let bytes = self
                .chunks
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|e| {
                    panic!(
                        "shell prompt {count}: {e}; transcript {:?}",
                        String::from_utf8_lossy(&self.transcript)
                    )
                });
            self.transcript.extend_from_slice(&bytes);
            self.session.feed(&bytes, Instant::now());
        }
    }
    /// Preserve the exact raw stream alongside the semantic assertions for review.
    fn evidence(&self) {
        fs::write(self.directory.join("transcript.bin"), &self.transcript)
            .expect("shell transcript");
    }
}
/// Source commands use an environment path, never interpolated untrusted shell code.
fn peer(name: &str) -> Peer {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .canonicalize()
        .expect("workspace");
    let directory = root
        .join("target/m9-shells")
        .join(format!("{name}-{}", std::process::id()));
    let cwd = directory.join("space ü");
    fs::create_dir_all(&cwd).expect("test directory");
    let mut command = shell_command(name, &directory);
    command.cwd(&cwd);
    command.env(
        "PUZZLE_SNIPPET",
        root.join("assets/shell-integration").join(match name {
            "bash" => "bash.sh",
            "zsh" => "zsh.zsh",
            _ => "fish.fish",
        }),
    );
    let spawned = spawn(
        command,
        PtySize {
            columns: 100,
            rows: 40,
            pixel_width: 1000,
            pixel_height: 800,
        },
    )
    .expect("interactive shell");
    let child = ChildGuard(spawned.child);
    let (tx, chunks) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = spawned.reader;
        let mut buffer = [0; 8192];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 || tx.send(buffer[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    Peer {
        session: Session::new(
            SessionConfig {
                columns: 100,
                rows: 40,
                ..Default::default()
            },
            Services::new(Box::new(spawned.transport), Box::new(FrameWaker)),
        ),
        chunks,
        transcript: Vec::new(),
        directory,
        _child: child,
    }
}
/// Startup files represent public framework hook seams, without installing frameworks.
fn shell_command(name: &str, directory: &Path) -> CommandBuilder {
    match name {
        "bash" => {
            fs::write(directory.join("bashrc"), "framework_debug_count=0; trap '((framework_debug_count+=1))' DEBUG\nPROMPT_COMMAND='framework_status=$?; PS1=\"[framework:$framework_status]$ \"'\nsource \"$PUZZLE_SNIPPET\"\nsource \"$PUZZLE_SNIPPET\"\n").expect("bashrc");
            let mut command = CommandBuilder::new("/bin/bash");
            command.args(["--noprofile", "--rcfile"]);
            command.arg(directory.join("bashrc"));
            command.arg("-i");
            command
        }
        "zsh" => {
            fs::write(directory.join(".zshrc"), "framework_hook_count=0\nframework_prompt() { local code=$?; ((framework_hook_count+=1)); PROMPT=\"[framework:$code]$ \"; }\nprecmd_functions=(framework_prompt)\nsource \"$PUZZLE_SNIPPET\"\nsource \"$PUZZLE_SNIPPET\"\n").expect("zshrc");
            let mut command = CommandBuilder::new("/bin/zsh");
            command.arg("-i");
            command.env("ZDOTDIR", directory);
            command
        }
        _ => {
            let path = if Path::new("/opt/homebrew/bin/fish").is_file() {
                "/opt/homebrew/bin/fish"
            } else {
                "fish"
            };
            let mut command = CommandBuilder::new(path);
            command.args(["--no-config", "-i"]);
            if name == "fish-legacy" {
                command.arg("--features=no-mark-prompt");
            }
            command.args(["--init-command", "function fish_prompt; set -l code $status; printf '[framework:%s]$ ' $code; end; source \"$PUZZLE_SNIPPET\"; source \"$PUZZLE_SNIPPET\""]);
            command
        }
    }
}
/// A complete command cycle is checked from parsed marks, not merely raw OSC bytes.
fn exercise(name: &str) {
    let mut peer = peer(name);
    peer.session.enable_host_events().expect("command facts");
    peer.prompt(1);
    let success = match name {
        "bash" => "printf 'success\\n'; test \"$framework_debug_count\" -gt 0\r",
        "zsh" => "printf 'success\\n'; test \"$framework_hook_count\" -gt 0\r",
        _ => "printf 'success\\n'\r",
    };
    for (index, command) in [success, "false\r", "printf 'first\\nsecond\\n'\r"]
        .into_iter()
        .enumerate()
    {
        peer.session
            .submit(Command::Text(command.into()), Origin::User);
        peer.session.pump(Instant::now());
        peer.prompt(index + 2);
    }
    peer.evidence();
    assert_commands(&mut peer, name, success);
    let view = peer.session.buffer();
    let marks: Vec<_> = view.zones().collect();
    let exits: Vec<_> = marks
        .iter()
        .filter(|z| z.kind == ZoneKind::Finished)
        .map(|z| z.exit)
        .collect();
    assert_eq!(exits, [Some(0), Some(1), Some(0)], "{name}");
    let outputs: Vec<_> = marks
        .iter()
        .filter(|z| z.kind == ZoneKind::Output)
        .map(|z| view.command_output(z).expect("closed output"))
        .collect();
    assert_eq!(outputs, ["success\n", "", "first\nsecond\n"], "{name}");
    assert_eq!(
        marks.iter().filter(|z| z.kind == ZoneKind::Prompt).count(),
        4,
        "double sourcing: {name}"
    );
    let uri = view.cwd().expect("OSC 7");
    assert!(uri.ends_with("/space%20%C3%BC"), "{name}: {uri}");
    // The framework still sees failure status instead of a helper's successful printf.
    assert!(
        String::from_utf8_lossy(&peer.transcript).contains("[framework:1]$"),
        "{name}: framework status lost"
    );
}
/// Coarse host facts carry labels from the real shell, not reconstructed grid text.
fn assert_commands(peer: &mut Peer, name: &str, success: &str) {
    let commands: Vec<_> = std::iter::from_fn(|| peer.session.take_event())
        .filter_map(|event| match event {
            puzzle_term::HostEvent::Fact(puzzle_term::messages::Event::CommandStarted(command)) => {
                Some(command)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        commands,
        [
            Some(success.trim_end_matches('\r').into()),
            Some("false".into()),
            Some("printf 'first\\nsecond\\n'".into())
        ],
        "command labels: {name}"
    );
}
/// macOS bash 3.2 preserves DEBUG/PROMPT_COMMAND and status.
#[test]
fn bash_snippet_real_pty_zones() {
    exercise("bash");
}
/// ZLE hook arrays preserve prompt frameworks and output boundaries.
#[test]
fn zsh_snippet_real_pty_zones() {
    exercise("zsh");
}
/// Modern fish owns OSC 133; integration must not duplicate its native marks.
#[test]
fn fish_snippet_real_pty_zones() {
    exercise("fish");
}
/// Feature-disabled fish exercises the explicit fallback hooks.
#[test]
fn fish_legacy_snippet_real_pty_zones() {
    exercise("fish-legacy");
}

/// Automatic startup uses the same shared constructor as both native hosts.
mod automatic;
