//! Native login shells load existing snippets without edits to user startup files.
//! Test homes are private workspace fixtures, never the developer's real home.
use super::*;
use std::ffi::{OsStr, OsString};

/// Builds framework-style startup files, optionally manually sourcing the same snippet.
fn startup(name: &str, directory: &Path, manual: bool) -> Vec<(OsString, OsString)> {
    let source = if manual {
        "source \"$PUZZLE_SNIPPET\"; source \"$PUZZLE_SNIPPET\""
    } else {
        ""
    };
    let mut env = vec![("HOME".into(), directory.as_os_str().to_owned())];
    match name {
        "bash" => {
            fs::write(directory.join(".bash_profile"), format!("printf 'STARTUP-PROFILE\n'\nPROMPT_COMMAND='framework_status=$?; PS1=\"[framework:$framework_status]$ \"'\n{source}\n")).expect("bash profile");
            // Only the first readable login file runs. A later profile must not be sourced.
            fs::write(directory.join(".bash_login"), "printf 'WRONG-PROFILE\n'\n")
                .expect("unused login");
            env.push(("ENV".into(), "original-env-value".into()));
        }
        "zsh" => {
            let user = directory.join("user startup");
            fs::create_dir_all(&user).expect("ZDOTDIR");
            for (file, marker) in [
                (".zshenv", "ZENV"),
                (".zprofile", "ZPROFILE"),
                (".zlogin", "ZLOGIN"),
            ] {
                fs::write(user.join(file), format!("printf '{marker}\n'\n")).expect("zsh startup");
            }
            fs::write(user.join(".zshrc"), format!("framework_hook_count=0\nframework_prompt() {{ local code=$?; ((framework_hook_count+=1)); PROMPT=\"[framework:$code]$ \"; }}\nprecmd_functions+=(framework_prompt)\n{source}\n")).expect("zshrc");
            env.push(("ZDOTDIR".into(), user.into_os_string()));
        }
        _ => {
            let config = directory.join("config/fish");
            fs::create_dir_all(&config).expect("fish config");
            fs::write(config.join("config.fish"), format!("printf 'STARTUP-CONFIG\n'\nfunction fish_prompt; set -l code $status; printf '[framework:%s]$ ' $code; end\n{source}\n")).expect("fish config");
            let vendor = directory.join("vendor/fish/vendor_conf.d");
            fs::create_dir_all(&vendor).expect("vendor conf");
            fs::write(vendor.join("existing.fish"), "printf 'STARTUP-VENDOR\n'\n")
                .expect("vendor config");
            env.push((
                "XDG_CONFIG_HOME".into(),
                directory.join("config").into_os_string(),
            ));
            env.push((
                "XDG_DATA_DIRS".into(),
                directory.join("vendor").into_os_string(),
            ));
        }
    }
    env
}

/// Starts through the same configured-login constructor that App uses for both hosts.
fn automatic_peer(name: &str, manual: bool, configured: bool) -> Peer {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace");
    let directory = root.join("target/m18/shell-integration").join(format!(
        "{name}-{manual}-{configured}-{}",
        std::process::id()
    ));
    let cwd = directory.join("space ü");
    fs::create_dir_all(&cwd).expect("fixture home");
    let mut env = if configured {
        startup(name, &directory, manual)
    } else {
        // No startup file contains integration or prompt setup in this fixture.
        vec![
            ("HOME".into(), directory.as_os_str().to_owned()),
            ("ZDOTDIR".into(), directory.as_os_str().to_owned()),
            (
                "XDG_CONFIG_HOME".into(),
                directory.join("empty-config").into_os_string(),
            ),
            (
                "XDG_DATA_DIRS".into(),
                directory.join("empty-data").into_os_string(),
            ),
        ]
    };
    let (shell, snippet) = match name {
        "bash" => ("/bin/bash", "bash.sh"),
        "zsh" => ("/bin/zsh", "zsh.zsh"),
        _ => ("/opt/homebrew/bin/fish", "fish.fish"),
    };
    env.push((
        "PUZZLE_SNIPPET".into(),
        root.join("assets/shell-integration")
            .join(snippet)
            .into_os_string(),
    ));
    let spawned = crate::pty::login::spawn_login_settings(
        PtySize {
            columns: 100,
            rows: 40,
            pixel_width: 1000,
            pixel_height: 800,
        },
        Appearance::Dark,
        Some(OsStr::new(shell)),
        Some(&cwd),
        &env,
        "detect",
    )
    .expect("automatic login shell");
    receive_peer(spawned, directory)
}

/// Owns the private shell and streams output through the ordinary Session parser.
fn receive_peer(spawned: crate::pty::SpawnedPty, directory: PathBuf) -> Peer {
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

/// Parsed marks prove working command boundaries, not just a printed capability string.
fn exercise_automatic(name: &str, manual: bool) {
    let mut peer = automatic_peer(name, manual, true);
    peer.prompt(1);
    for (index, command) in [
        "printf 'success\\n'\r",
        "false\r",
        "printf 'first\\nsecond\\n'\r",
    ]
    .into_iter()
    .enumerate()
    {
        peer.session
            .submit(Command::Text(command.into()), Origin::User);
        peer.session.pump(Instant::now());
        peer.prompt(index + 2);
    }
    peer.evidence();
    let view = peer.session.buffer();
    let marks: Vec<_> = view.zones().collect();
    assert_eq!(
        marks
            .iter()
            .filter(|z| z.kind == ZoneKind::Finished)
            .map(|z| z.exit)
            .collect::<Vec<_>>(),
        [Some(0), Some(1), Some(0)]
    );
    assert_eq!(
        marks
            .iter()
            .filter(|z| z.kind == ZoneKind::Output)
            .map(|z| view.command_output(z).expect("closed output"))
            .collect::<Vec<_>>(),
        ["success\n", "", "first\nsecond\n"]
    );
    assert_eq!(
        marks.iter().filter(|z| z.kind == ZoneKind::Prompt).count(),
        4,
        "duplicate injection/manual marks: {name}"
    );
    assert!(view.cwd().expect("OSC 7").ends_with("/space%20%C3%BC"));
    let raw = String::from_utf8_lossy(&peer.transcript);
    assert!(
        raw.contains("[framework:1]$"),
        "framework lost status: {raw}"
    );
    verify_startup(name, &raw);
}

/// Existing startup output and vendor/profile ordering survive the injection seam.
fn verify_startup(name: &str, raw: &str) {
    let startup = match name {
        "bash" => "STARTUP-PROFILE",
        "zsh" => "ZPROFILE",
        _ => "STARTUP-CONFIG",
    };
    assert!(raw.contains(startup), "startup file skipped: {raw}");
    assert!(
        !raw.contains("WRONG-PROFILE"),
        "bash sourced more than its first login file"
    );
    if name == "zsh" {
        assert!(raw.contains("ZENV") && raw.contains("ZLOGIN"));
    }
    if name == "fish" {
        assert!(raw.contains("STARTUP-VENDOR"));
    }
}

/// macOS bash 3.2 must load its profile before installing prompt and DEBUG hooks.
#[test]
fn integration_bash_real_login() {
    if !crate::pty::integration::supports_bash(&CommandBuilder::new("/bin/bash")) {
        // User-approved Apple 3.2 exception retains manual sourcing in its real
        // login profile, including coexistence with the idempotent snippets.
        exercise_automatic("bash", true);
        return;
    }
    for manual in [false, true] {
        exercise_automatic("bash", manual);
    }
}
/// Restoring ZDOTDIR preserves all four user files and framework hook arrays.
#[test]
fn integration_zsh_real_login() {
    for manual in [false, true] {
        exercise_automatic("zsh", manual);
    }
}
/// Fish 4.9.3 keeps vendor/user configuration and its native OSC 133 behavior.
#[test]
fn integration_fish_real_login() {
    for manual in [false, true] {
        exercise_automatic("fish", manual);
    }
}

/// Empty homes need no user sourcing or startup configuration to produce both protocols.
#[test]
fn integration_unconfigured_shells_real_login() {
    for name in ["bash", "zsh", "fish"] {
        if name == "bash"
            && !crate::pty::integration::supports_bash(&CommandBuilder::new("/bin/bash"))
        {
            continue; // The Apple exception is independently checked below.
        }
        let mut peer = automatic_peer(name, false, false);
        peer.prompt(1);
        peer.session
            .submit(Command::Text("false\r".into()), Origin::User);
        peer.session.pump(Instant::now());
        peer.prompt(2);
        peer.evidence();
        let view = peer.session.buffer();
        assert!(
            view.cwd()
                .expect("initial OSC 7")
                .ends_with("/space%20%C3%BC")
        );
        assert_eq!(
            view.zones()
                .filter(|zone| zone.kind == ZoneKind::Prompt)
                .count(),
            2
        );
        assert_eq!(
            view.zones()
                .filter(|zone| zone.kind == ZoneKind::Finished)
                .map(|zone| zone.exit)
                .collect::<Vec<_>>(),
            [Some(1)]
        );
    }
}

/// Unsupported Apple bash keeps its true login flag, ENV and normal profile.
#[test]
fn integration_apple_bash_preserves_login_startup() {
    if crate::pty::integration::supports_bash(&CommandBuilder::new("/bin/bash")) {
        return;
    }
    let mut peer = automatic_peer("bash", false, true);
    wait_text(&mut peer, "[framework:0]$");
    peer.session.submit(
        Command::Text(
            "printf 'LOGIN:%s ENV:%s\\n' \"$(shopt -q login_shell; printf '%s' $?)\" \"$ENV\"\r"
                .into(),
        ),
        Origin::User,
    );
    peer.session.pump(Instant::now());
    wait_text(&mut peer, "LOGIN:0 ENV:original-env-value");
    assert_eq!(peer.session.buffer().zones().count(), 0);
    peer.evidence();
}

/// Functional startup receipts use the same generous deadlock guard as manual tests.
fn wait_text(peer: &mut Peer, text: &str) {
    let deadline = Instant::now() + crate::test_support::HANG_GUARD;
    while !String::from_utf8_lossy(&peer.transcript).contains(text) {
        let bytes = peer
            .chunks
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("shell receipt");
        peer.transcript.extend_from_slice(&bytes);
        peer.session.feed(&bytes, Instant::now());
    }
}
