//! Real ZLE SIGWINCH redraws must not resurrect a two-line prompt from history.
//! The fixture has no user startup files or input after startup. A leading newline
//! models Starship's add_newline without installing a prompt framework.
use super::*;
use puzzle_term::vt::Terminal;

/// Own the real PTY and join its reader even if a prompt assertion fails.
struct Shell {
    /// Core state is identical to the state driven by a Session owner.
    terminal: Terminal,
    /// Startup output stays above the prompt in the user's second reproduction.
    startup: bool,
    /// Native resize sends SIGWINCH after the grid has adopted its new height.
    transport: Option<LocalPty>,
    /// The shell is reaped before the test's temporary startup files are removed.
    child: Box<dyn Child + Send + Sync>,
    /// Each native read is fed unchanged, preserving escape-sequence splits.
    chunks: mpsc::Receiver<Vec<u8>>,
    /// No reader thread is left detached after the test.
    reader: Option<thread::JoinHandle<()>>,
    /// Retain all shell bytes for a useful failure receipt.
    transcript: Vec<u8>,
}

impl Shell {
    /// Quiet is only a synchronization boundary, never a speed assertion.
    fn drain(&mut self, initial: bool) {
        let mut timeout = if initial {
            crate::test_support::HANG_GUARD
        } else {
            Duration::from_millis(100)
        };
        loop {
            match self.chunks.recv_timeout(timeout) {
                Ok(bytes) => {
                    self.transcript.extend_from_slice(&bytes);
                    self.terminal.advance(&bytes);
                    // Zsh may ask for cursor position while initializing ZLE.
                    if !self.terminal.replies.is_empty() {
                        self.transport
                            .as_mut()
                            .expect("live PTY")
                            .write(&self.terminal.replies)
                            .expect("query reply");
                        self.terminal.replies.clear();
                    }
                    timeout = Duration::from_millis(100);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(error) => panic!("zsh exited during resize: {error}"),
            }
        }
    }

    /// Compare complete rows and history after each SIGWINCH, not just raw prompt bytes.
    fn assert_one_prompt(&self) {
        let ring = &self.terminal.primary.ring;
        let lines: Vec<String> = ring
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| char::from_u32(cell.ch).unwrap_or(' '))
                    .collect()
            })
            .collect();
        let history = lines[..ring.scrollback_len()].join("\n");
        let screen = lines[ring.scrollback_len()..].join("\n");
        assert_eq!(
            screen.matches("[resize-directory]").count(),
            1,
            "{screen:?}"
        );
        assert_eq!(screen.matches("[resize-prompt]>").count(), 1, "{screen:?}");
        assert!(!history.contains("[resize-directory]"), "{history:?}");
        assert!(!history.contains("[resize-prompt]>"), "{history:?}");
        if self.startup {
            assert_eq!(
                screen.matches("Unsetting -CERT- vars").count(),
                1,
                "{screen:?}"
            );
            assert!(!history.contains("Unsetting -CERT- vars"), "{history:?}");
        }
    }

    /// Grid adoption precedes the native signal, exactly as in puzzle-embed.
    fn resize(&mut self, rows: usize) {
        self.terminal.resize(80, rows);
        self.transport
            .as_mut()
            .expect("live PTY")
            .resize(PtySize {
                columns: 80,
                rows: rows as u16,
                pixel_width: 800,
                pixel_height: rows as u16 * 20,
            })
            .expect("PTY resize");
        self.drain(false);
        self.assert_one_prompt();
    }
}

impl Drop for Shell {
    /// Cancel reading before reaping; no slave EOF is needed to join the reader.
    fn drop(&mut self) {
        if let Some(mut transport) = self.transport.take() {
            let _ = transport.close();
            drop(transport);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

/// Start a real interactive zsh with a deterministic multiline prompt.
fn shell(directory: &Path, add_newline: bool, startup: bool) -> Shell {
    fs::create_dir_all(directory).expect("fixture directory");
    let prefix = if add_newline { "\\n" } else { "" };
    let output = if startup {
        "printf 'Unsetting -CERT- vars ✌️\\n'\n"
    } else {
        ""
    };
    fs::write(
        directory.join(".zshrc"),
        format!("{output}PROMPT=$'{prefix}[resize-directory]\\n[resize-prompt]> '\nRPROMPT=''\n"),
    )
    .expect("zshrc");
    let mut command = CommandBuilder::new("/bin/zsh");
    command.arg("-i");
    command.env("ZDOTDIR", directory);
    let spawned = spawn(
        command,
        PtySize {
            columns: 80,
            rows: 24,
            pixel_width: 800,
            pixel_height: 480,
        },
    )
    .expect("zsh PTY");
    let (send, chunks) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut source = spawned.reader;
        let mut bytes = [0; 8192];
        while let Ok(count) = source.read(&mut bytes) {
            if count == 0 || send.send(bytes[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    Shell {
        terminal: Terminal::new(80, 24, 1000, 1 << 20),
        startup,
        transport: Some(spawned.transport),
        child: spawned.child,
        chunks,
        reader: Some(reader),
        transcript: Vec::new(),
    }
}

/// Twenty SIGWINCH events per prompt variant, without any key or pasted input.
#[test]
fn zsh_two_line_prompt_ten_vertical_resize_cycles_without_input() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/resize-primary");
    for (add_newline, startup) in [(false, false), (true, false), (true, true)] {
        let directory = root.join(format!(
            "zsh-{}-{add_newline}-{startup}",
            std::process::id()
        ));
        let mut shell = shell(&directory, add_newline, startup);
        shell.drain(true);
        shell.assert_one_prompt();
        for _ in 0..10 {
            shell.resize(5);
            shell.resize(24);
        }
        // One initial prompt plus twenty redraws proves SIGWINCH exercised ZLE.
        // Zsh uses ED (not necessarily EL) when redrawing a multiline prompt.
        fs::write(directory.join("transcript.bin"), &shell.transcript).expect("trace");
        assert_eq!(
            shell
                .transcript
                .windows(b"[resize-prompt]>".len())
                .filter(|bytes| *bytes == b"[resize-prompt]>")
                .count(),
            21,
            "one initial prompt and one ZLE redraw per native resize"
        );
    }
}
