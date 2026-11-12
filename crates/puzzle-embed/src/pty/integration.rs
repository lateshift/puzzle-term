//! Startup-only shell integration shared by every native PTY host (M18.8).
//!
//! Only login-shell construction calls this module; arbitrary child commands and
//! host transports are untouched. The shell loads our existing snippets after
//! its own startup files, without changing any file in the user's home directory.
//! Resource lookup and environment edits happen once before spawn, never in VT.
use portable_pty::CommandBuilder;
use std::{ffi::OsStr, path::Path};

/// Supported startup mechanisms are explicit, rather than inferred from TERM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shell {
    /// POSIX ENV bootstrap restores normal bash login startup before our snippet.
    Bash,
    /// A temporary ZDOTDIR restores the real directory from its .zshenv.
    Zsh,
    /// A vendor_conf.d entry defers installation until user configuration finishes.
    Fish,
}

/// Selects only the configured executable's basename; unknown shells stay untouched.
fn select(executable: &OsStr, setting: &str) -> Option<Shell> {
    // Explicit choices deliberately override basename detection for wrapper executables.
    let name = if setting == "detect" {
        // Unix executable names are case-sensitive; a similarly named custom
        // shell must not accidentally receive another shell's startup grammar.
        Path::new(executable).file_name()?.to_str()?
    } else {
        setting
    };
    match name {
        // Keep the recognized set closed. In particular, sh may be dash or a
        // compatibility mode whose startup semantics differ from native bash.
        "bash" => Some(Shell::Bash),
        "zsh" => Some(Shell::Zsh),
        "fish" => Some(Shell::Fish),
        _ => None,
    }
}

/// Preserves unset versus empty values without interpolating paths into shell code.
fn preserve(command: &mut CommandBuilder, original: &str, saved: &str) {
    // CommandBuilder sees host overrides and inherited values through one merged view.
    let value = command.get_env(original).map(ToOwned::to_owned);
    // A nested Puzzle process must not restore a stale private value from its
    // parent when the current child deliberately has the public variable unset.
    command.env_remove(saved);
    if let Some(value) = value {
        command.env(saved, value);
    }
}

/// Installs the selected startup route before spawning a native Unix login shell.
pub(super) fn apply(command: &mut CommandBuilder, setting: &str) {
    apply_with(command, setting, supports_bash);
}

/// Assesses the selected executable once; test fixtures supply known version outcomes.
fn apply_with(
    command: &mut CommandBuilder,
    setting: &str,
    bash_supported: impl FnOnce(&CommandBuilder) -> bool,
) {
    // A malformed/empty builder has no executable to classify; startup remains
    // the ordinary constructor's responsibility rather than causing a panic here.
    let Some(executable) = command.get_argv().first() else {
        return;
    };
    let Some(shell) = select(executable, setting) else {
        // Disabled and unknown shells receive no resource variables or argument changes.
        tracing::debug!(event = "shell.integration_disabled", setting);
        return;
    };
    if shell == Shell::Bash && !bash_supported(command) {
        // Apple bash 3.2 cannot load ENV. Preserve its true login startup and
        // manual snippet rather than silently replacing it with a non-login shell.
        tracing::debug!(
            event = "shell.integration_unsupported",
            ?executable,
            "bash version cannot use ENV injection; manual sourcing remains available"
        );
        return;
    }
    // M12 packaging will relocate the shared resources; today's app and harness
    // intentionally use the same checkout resource directory as their terminfo.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/shell-integration");
    let wrapper = match shell {
        Shell::Bash => root.join("bash/bootstrap.sh"),
        Shell::Zsh => root.join("zsh/.zshenv"),
        Shell::Fish => root.join("fish/vendor_conf.d/puzzle.fish"),
    };
    if !wrapper.is_file() {
        // A missing resource must never prevent the user's shell from starting.
        tracing::warn!(event = "shell.integration_missing", ?wrapper);
        return;
    }
    // Pass the root as an environment value, never as evaluated shell source.
    // Wrappers quote it when sourcing the existing integration snippet.
    command.env("PUZZLE_SHELL_INTEGRATION", &root);
    match shell {
        Shell::Bash => bash(command, &wrapper),
        Shell::Zsh => {
            // Only the first .zshenv comes from us. It restores ZDOTDIR before
            // zsh chooses the user's remaining login and interactive files.
            preserve(command, "ZDOTDIR", "PUZZLE_SAVED_ZDOTDIR");
            command.env("ZDOTDIR", root.join("zsh"));
        }
        Shell::Fish => fish(command, &root),
    }
    tracing::debug!(event = "shell.integration_injected", ?shell);
}

/// Adds our vendor root while retaining fish's normal system/user discovery paths.
fn fish(command: &mut CommandBuilder, root: &Path) {
    preserve(command, "XDG_DATA_DIRS", "PUZZLE_SAVED_XDG_DATA_DIRS");
    // XDG's unset/empty value has standard defaults. Prepending only our
    // vendor directory leaves all system/vendor configuration discoverable.
    let previous = command
        .get_env("XDG_DATA_DIRS")
        .filter(|value| !value.is_empty())
        .unwrap_or(OsStr::new("/usr/local/share:/usr/share"));
    let mut paths = root.as_os_str().to_os_string();
    // Fish appends fish/vendor_conf.d to each XDG root. Prepending the
    // parent root, rather than that final directory, preserves discovery.
    paths.push(":");
    paths.push(previous);
    command.env("XDG_DATA_DIRS", paths);
}

/// Uses bash's POSIX ENV entry point, then the wrapper restores normal login semantics.
fn bash(command: &mut CommandBuilder, wrapper: &Path) {
    preserve(command, "ENV", "PUZZLE_SAVED_ENV");
    command.env("ENV", wrapper);
    // Long options precede -l. No command string or quoting parser is introduced.
    command.get_argv_mut().insert(1, "--posix".into());
    command.env_remove("PUZZLE_UNEXPORT_HISTFILE");
    if command.get_env("HISTFILE").is_none()
        && let Some(home) = command.get_env("HOME")
    {
        // POSIX bash otherwise selects .sh_history; preserve native bash's default
        // without exporting a newly synthesized history path to nested children.
        let history = Path::new(home).join(".bash_history");
        command.env("HISTFILE", history);
        command.env("PUZZLE_UNEXPORT_HISTFILE", "1");
    }
}

/// Environment and executable-name cases have no dependency on installed shells.
#[cfg(test)]
mod tests;

/// Reads the executable's version, never identifies an Apple binary from its path.
pub(super) fn supports_bash(command: &CommandBuilder) -> bool {
    let Some(executable) = command.get_argv().first() else {
        return false;
    };
    // --version exits before startup files. Resolve the same configured PATH
    // and cwd as the child, including a relative executable or wrapper path.
    let mut probe = std::process::Command::new(executable);
    probe.arg("--version").env("LC_ALL", "C");
    if let Some(path) = command.get_env("PATH") {
        probe.env("PATH", path);
    }
    if let Some(cwd) = command.get_cwd() {
        probe.current_dir(cwd);
    }
    // Probe only at spawn: neither per-command hooks nor parser/frame hot paths
    // execute a version subprocess. A failed probe leaves startup untouched.
    let output = probe.output();
    match output {
        Ok(output) if output.status.success() => {
            env_capable(&output.stdout, cfg!(target_os = "macos"))
        }
        _ => false,
    }
}

/// Apple's Darwin bash 3.2 patch disables ENV; modern GNU bash retains the entry.
fn env_capable(version: &[u8], macos: bool) -> bool {
    // --version is machine-selected output, but wrappers can emit arbitrary
    // bytes. Unrecognized text must not opt a shell into POSIX startup changes.
    let Ok(text) = std::str::from_utf8(version) else {
        return false;
    };
    let Some(version) = text
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("GNU bash, version "))
    else {
        return false;
    };
    // Patch/build suffixes vary across vendors; the major/minor pair identifies
    // Apple's unsupported line without tying the decision to a filesystem path.
    let mut parts = version.split('.');
    let (Some(major), Some(minor)) = (parts.next(), parts.next()) else {
        return false;
    };
    let (Ok(major), Ok(minor)) = (major.parse::<u32>(), minor.parse::<u32>()) else {
        return false;
    };
    // Version classification is intentional: /bin/bash paths can be symlinks on
    // other systems, and a copied Apple 3.2 binary must still receive the exception.
    !(macos && major == 3 && minor == 2)
}
