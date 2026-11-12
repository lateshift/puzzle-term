//! Child-process orchestration for reproducible M0 checks (design §2.3, R14).
//!
//! `xtask` is an ordinary workspace binary. Each public subcommand invokes
//! the same compiler or harness command a developer can run by hand. Child
//! failures become explicit `Err` values, allowing the top-level CLI to exit
//! nonzero instead of printing a success message after a failed check.
//!
//! There are two output modes. Interactive commands inherit their output so
//! build progress and window errors remain visible. Gates capture successful
//! output so acceptance can report each criterion on one line while retaining
//! the failing command's diagnostics.
//!
//! Trunk 0.21 finds its Cargo root from the process working directory. Since
//! the repository root is a virtual workspace, Trunk runs from the web crate
//! and reads `web/index.html` through a relative path. Cargo commands still
//! run from the workspace root, where the lockfile and xtask alias live.
//!
//! The wasm size report measures the shipped Trunk output after wasm-opt,
//! not the intermediate Cargo `.wasm`. gzip and brotli bytes are counted from
//! compressor stdout to avoid rounding the budget at display time.
//!
//! The interactive `run` shortcut uses Cargo's optimized live profile
//! because a debug-profile 300×80 redraw can consume a display interval.
//! `--debug` remains explicit for source-level debugging. Log-file options
//! are passed after Cargo's `--`, where puzzle-desktop rather than Cargo
//! parses them. Keeping the argument builder pure makes this boundary
//! testable without launching a window or writing into a real user's log.
//!
//! The requested log path remains a borrowed argument until the child
//! command starts. This avoids shell expansion: spaces and punctuation
//! are forwarded as one OS argument, not interpolated into a command line.
use std::{
    fs,
    process::{Command, Stdio},
};
/// Scenario discovery and committed-reference comparisons for §13.1.
pub(crate) mod harness;
/// Acceptance-only native feature combinations are checked without workspace unification.
mod native_features;
/// Optional feature gates are an independent distribution component.
mod optional_checks;
/// Configures argv and Trunk's package root while inheriting the user's environment.
///
/// Both output modes share this setup: neither may inject NO_COLOR=false,
/// whose nonempty value disables color in programs inside desktop/bridge PTYs.
/// An unset, empty or user-supplied NO_COLOR must reach ordinary children intact.
/// Trunk alone needs its boolean spelling adapted from the NO_COLOR convention.
fn child_command(program: &str, args: &[&str]) -> Command {
    let mut command = Command::new(program);
    command.args(args);
    // Trunk's metadata lookup needs a package cwd; Cargo needs the workspace root.
    if program == "trunk" {
        command.current_dir("crates/puzzle-web");
        // Trunk's clap boolean rejects conventional values such as 1. Adapt
        // only its process: every nonempty user value means colors disabled.
        // If absent, keep it absent; the ordinary Cargo/PTY paths never enter here.
        if let Some(value) = std::env::var_os("NO_COLOR") {
            command.env("NO_COLOR", if value.is_empty() { "false" } else { "true" });
        }
    }
    command
}

/// Runs a command with inherited output and checks its exit status.
///
/// This path is for a human watching a build or an interactive server. Trunk
/// receives the package working directory required by its metadata lookup.
fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let mut command = child_command(program, args);
    let status = command
        .status()
        .map_err(|error| format!("{program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {} failed: {status}", args.join(" ")))
    }
}
/// Runs a command quietly, returning its stderr if it fails.
///
/// A gate needs the first failing subcommand's diagnostic, while successful
/// compiler chatter would obscure the single acceptance result line.
pub(crate) fn quiet(program: &str, args: &[&str]) -> Result<(), String> {
    capture(program, args).map(|_| ())
}

/// Captures a successful command's stdout for measured acceptance criteria.
pub(crate) fn capture(program: &str, args: &[&str]) -> Result<String, String> {
    // Keep compiler chatter out of the per-criterion acceptance report.
    let mut command = child_command(program, args);
    let output = command
        .output()
        .map_err(|error| format!("{program}: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        // Rust tests put assertion details on stdout, while Cargo's stderr
        // only names the failed binary. Preserve both to diagnose a gate
        // failure instead of silently losing its actual assertion evidence.
        Err(format!(
            "{program} {} failed:\n{}{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}
/// Applies formatting, native/wasm Clippy, tests, and the repository linter.
///
/// Native Clippy covers every workspace package and target, including xtask
/// and the window binary. Its wasm invocation names only the browser crate
/// and platform-neutral crates; the native shell cannot compile for wasm.
/// Both invocations treat warnings as errors, including missing private docs.
/// Tests run after compilation so an accidental failing test cannot be hidden
/// by a successful lint scan. The final source scan enforces project rules
/// that Rust and Clippy do not know, including architecture boundaries and
/// the workspace-wide comment quality guard.
pub(crate) fn check() -> Result<(), String> {
    // The web distribution is client-only even when a local bridge was built.
    crate::package::verify()?;
    // Keep errors visible but successful inner commands quiet for the acceptance gate.
    quiet("cargo", &["fmt", "--check"])?;
    quiet(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    native_features::check()?;
    // The desktop shell is deliberately excluded: only the web shell and neutral crates target wasm.
    quiet(
        "cargo",
        &[
            "clippy",
            "--target",
            "wasm32-unknown-unknown",
            "-p",
            "puzzle-bus",
            "-p",
            "puzzle-vt",
            "-p",
            "puzzle-slug",
            "-p",
            "puzzle-text",
            "-p",
            "puzzle-frame",
            "-p",
            "puzzle-overlay",
            "-p",
            "puzzle-term",
            "-p",
            "puzzle-render",
            "-p",
            "puzzle-web",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    quiet("cargo", &["test", "--workspace"])?;
    optional_checks::check()?;
    crate::ffi::verify()?;
    crate::terminfo::verify()?;
    crate::themes::verify()?;
    crate::lint::run()
}
/// Launches the live shell with optimized frame assembly by default.
///
/// Debug is explicit because a dev-profile full tmux frame can occupy most
/// of one display interval even when the parser and renderer are concurrent.
pub(crate) fn desktop_run(options: &[String]) -> Result<(), String> {
    // Show privacy guidance without compiling or starting the graphical
    // child; users often ask for the log syntax before opening a window.
    if options == ["--help"] {
        println!(
            "cargo xtask run [--debug] [--config PATH] [--profile NAME] [--set key=value] [--list-keybindings] [--list-actions] [--log-file PATH] [--record [FILE.pzrec]]\nRecordings contain everything typed and shown. The default path is target/recordings/.\nPUZZLE_VERIFY_FRAMES=1 checks cold redraws. A synthetic check took about 15 ms to build and 15 ms to render/read back; the live verify-mode gaps remain unexplained. Judge pacing in a normal run."
        );
        return Ok(());
    }
    // Cargo receives a vector of arguments directly; no shell is involved
    // in the privacy-sensitive destination path or debug switch.
    let args: Vec<&str> = options.iter().map(String::as_str).collect();
    let args = live_run_args(&args)?;
    run("cargo", &args)
}

/// Keeps the run-profile choice testable without opening an OS window.
fn live_run_args<'a>(options: &'a [&'a str]) -> Result<Vec<&'a str>, String> {
    let mut args = vec!["run"];
    // Profile selection precedes the package and its `--` separator;
    // placing it afterward would send a Cargo option to puzzle-desktop.
    let debug = options.contains(&"--debug");
    if !debug {
        args.extend(["--profile", "live"]);
    }
    args.extend(["-p", "puzzle-desktop", "--", "run"]);
    // Native flags follow the binary mode. The loop accepts both option
    // orders while rejecting an unknown flag before opening a window.
    let mut index = 0;
    while index < options.len() {
        match options[index] {
            "--debug" => index += 1,
            "--list-keybindings" | "--list-actions" => {
                // Headless introspection still uses the native option parser.
                args.push(options[index]);
                index += 1;
            }
            "--log-file" | "--config" | "--profile" | "--set" => {
                // Paths with spaces stay one argument; missing values fail
                // here before starting a child or creating a log file.
                let flag = options[index];
                let value = options
                    .get(index + 1)
                    .ok_or(format!("{flag} needs a value"))?;
                args.extend([flag, value]);
                index += 2;
            }
            "--record" => {
                // A following option is not a path. This supports the
                // timestamped default while preserving either flag order.
                // Forward the choice unchanged so the native binary owns
                // private-file creation before it starts the PTY.
                args.push("--record");
                let path = options
                    .get(index + 1)
                    .copied()
                    .filter(|value| !value.starts_with("--"));
                if let Some(path) = path {
                    args.push(path);
                }
                index += if path.is_some() { 2 } else { 1 };
            }
            value => return Err(format!("unknown run option: {value}")),
        }
    }
    Ok(args)
}

/// Serves or builds the Trunk page from web/index.html.
///
/// Serving binds to loopback for a local browser review. Building uses the
/// release profile in the virtual workspace, then reports all three size
/// forms from the final artifact in `dist/web`. The demo HTML itself remains
/// in `web/`, separate from the Rust crate that owns DOM and WebGPU setup.
pub(crate) fn web(mode: Option<&str>) -> Result<(), String> {
    match mode {
        // This diagnostic page and its output are excluded from web/index.html and dist/pkg/.
        // Fixture bytes and benchmark bindings exist only in this explicit dev build.
        Some("serve-bench") => run(
            "trunk",
            &[
                "serve",
                "../../xtask/fixtures/benchmark.html",
                "--release",
                "--features",
                "measurements",
                "--dist",
                "../../target/browser-bench",
                "--address",
                "127.0.0.1",
                "--port",
                "8092",
            ],
        ),
        Some("serve-stress") => run(
            "trunk",
            &[
                "serve",
                "../../xtask/fixtures/browser-stress.html",
                "--release",
                "--dist",
                "../../target/browser-stress",
                "--address",
                "127.0.0.1",
                "--port",
                "8089",
            ],
        ),
        Some("serve") => run(
            "trunk",
            &[
                "serve",
                "../../web/index.html", // Resolve the page from the web package's cwd.
                "--address",
                "127.0.0.1",
                "--port",
                "8080",
                "--features",
                "review-scenes",
            ],
        ),
        Some("test") => {
            // The same suite serves acceptance and direct browser debugging.
            println!("{}", crate::browser::suite()?);
            Ok(())
        }
        Some("build") => build_web(false),
        Some("build-review") => build_web(true),
        _ => Err(
            "usage: cargo xtask web <serve|serve-stress|serve-bench|test|build|build-review>"
                .into(),
        ),
    }
}

/// Measures either the full shipped terminal or the optional historical review URLs.
fn build_web(review: bool) -> Result<(), String> {
    let dist = if review {
        "dist/web-review"
    } else {
        "dist/web"
    };
    let mut args = vec!["build", "../../web/index.html", "--release", "--dist"];
    args.push(if review {
        "../../dist/web-review"
    } else {
        "../../dist/web"
    });
    if review {
        // The Slug and text review scenes remain available to M3/M4 checks,
        // while the shipped bundle excludes their fixture preparation code.
        args.extend(["--features", "review-scenes"]);
    }
    run("trunk", &args)?;
    // Inspect the built artifact rather than trusting Trunk's exit code alone.
    let wasm = fs::read_dir(dist)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "wasm"))
        .ok_or("Trunk did not emit a wasm file")?;
    crate::web_types::emit(&wasm)?;
    let raw = fs::metadata(&wasm)
        .map_err(|error| error.to_string())?
        .len();
    let gzip = compressed("gzip", &["-c", wasm.to_str().ok_or("non-UTF-8 wasm path")?])?;
    let brotli = compressed(
        "brotli",
        &["-c", wasm.to_str().ok_or("non-UTF-8 wasm path")?],
    )?;
    println!("wasm bytes: raw={raw} gzip={gzip} brotli={brotli}");
    Ok(())
}
/// Counts the bytes produced by an installed compression tool.
///
/// The measurement is the complete compressed stream size, including its
/// header. This is reproducible without adding a compression crate to xtask.
pub(crate) fn compressed(program: &str, args: &[&str]) -> Result<usize, String> {
    // Size counts use the exact compressor bytes, not a rounded display.
    let output = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .output()
        .map_err(|error| format!("{program}: {error}"))?;
    if output.status.success() {
        Ok(output.stdout.len())
    } else {
        Err(format!("{program} failed"))
    }
}

#[cfg(test)]
/// CLI argument tests stay separate from process orchestration.
mod tests;

/// Launches the native bridge with argv passed directly, without shell expansion.
pub(crate) fn bridge(options: &[String]) -> Result<(), String> {
    // The bridge owns validation and prints its secret URL only at startup.
    let mut args = vec!["run", "--profile", "live", "-p", "puzzle-pty-bridge", "--"];
    args.extend(options.iter().map(String::as_str));
    run("cargo", &args)
}
