//! Local-only coverage-guided runs in an isolated nightly workspace (M19.2b).
//! The root workspace never links libfuzzer-sys/arbitrary. Mutable corpora,
//! crash artifacts and sanitizer objects all stay under target/, while small
//! reviewed seeds live in fuzz/seeds. The caller chooses a finite duration;
//! fuzzing is intentionally not part of check or an absolute performance gate.
//!
//! There are three persistent locations: checked-in seeds establish known
//! behavior, a mutable target/ corpus retains newly covered paths, and a separate
//! target/ artifact directory preserves failures. They are never interchanged.
//! The sanitizer compiler's target directory is also explicit: crate-local
//! Cargo output would mix ordinary builds with incompatible instrumented code.
//!
//! rustup selects the compiler before Cargo starts, avoiding dependence on the
//! executable that happened to launch xtask. Debug assertions are explicit so
//! the OSC 66 ownership oracle is present in optimized fuzzing binaries.
//! A crash status propagates unchanged and leaves the reproducer for a normal
//! regression test; the driver never retries, minimizes or fixes a finding.
//! Only an explicit fuzz command launches the engine. Ordinary check validates
//! these paths and CLI semantics without performing random-duration work.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

/// One pinned compiler keeps sanitizer/coverage objects mutually compatible.
const NIGHTLY: &str = "nightly-2026-08-14";
// Names select checked-in targets, never arbitrary paths or commands.
// The catalogue alone cannot start a campaign in check or acceptance.
include!("fuzz/targets.rs");
/// design §6.4's outer OSC cap reaches truncation without unbounded fuzz allocations.
const MAX_INPUT_BYTES: usize = 65_536;

/// Builds a finite local fuzz run, preserving every finding in target/fuzz.
pub(crate) fn run(args: &[String]) -> Result<(), String> {
    // An exact argument shape prevents trailing user text becoming fuzzer flags.
    // Finite duration is mandatory: an omitted value must not launch an endless job.
    let [target, flag, duration] = args else {
        return Err("usage: cargo xtask fuzz <target> --time <seconds|Ns|Nm|Nh>".into());
    };
    // Restrict target names before joining paths, excluding traversal or a
    // binary outside the checked-in fuzz workspace. No shell executes these args.
    if !TARGETS.contains(&target.as_str()) || flag != "--time" {
        return Err("unknown fuzz target or missing --time".into());
    }
    let seconds = seconds(duration)?;
    // Derive paths from the compiled xtask location, not a caller-selected cwd.
    // A command launched from a crate still writes into the workspace target/.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("workspace root missing")?;
    let corpus = seed(root, target)?;
    // A failed input remains available even if a later run is interrupted.
    // Corpora and artifacts are distinct: minimizing must never erase the finding.
    let artifacts = root.join("target/fuzz/artifacts").join(target);
    fs::create_dir_all(&artifacts).map_err(|error| error.to_string())?;
    let mut command = invocation(root, target, seconds, &corpus, &artifacts);
    let log = capture_output(root, target, &mut command)?;
    println!(
        "FUZZ {target}: {seconds}s, {NIGHTLY}, ASan + debug invariants; output {}",
        log.display()
    );
    // Raw engine output may include untrusted bytes from a finding. Keep it
    // entirely in the file; the caller receives paths/status, never input text.
    let status = command
        .status()
        .map_err(|error| format!("start cargo-fuzz: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "fuzz {target} failed: {status}; output {}; artifacts {}",
            log.display(),
            artifacts.display()
        ))
    }
}

/// Redirects both compiler and engine streams before spawning either process.
fn capture_output(root: &Path, target: &str, command: &mut Command) -> Result<PathBuf, String> {
    // Each invocation keeps its own transcript, even when an earlier attempt
    // was interrupted. Wall time is a filename identity, not a policy timer.
    let directory = root.join("target/m19-fuzz-runs");
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let path = directory.join(format!("{target}-{stamp}-{}.log", std::process::id()));
    // Never truncate an existing receipt; a naming collision is an error.
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| error.to_string())?;
    // Clones share the same open file offset, preserving one combined stream.
    let stderr = file.try_clone().map_err(|error| error.to_string())?;
    command
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(stderr));
    Ok(path)
}

/// Builds argv without shell interpolation and isolates all sanitizer artifacts.
fn invocation(root: &Path, target: &str, seconds: u64, corpus: &Path, artifacts: &Path) -> Command {
    // rustup run selects the actual compiler, even when xtask itself was
    // started by a direct toolchain Cargo that would ignore +nightly dispatch.
    let build = root.join("target/fuzz/build");
    let mut command = Command::new("rustup");
    command
        .args([
            "run",
            NIGHTLY,
            "cargo",
            "fuzz",
            "run",
            "--debug-assertions",
            "--sanitizer",
            "address",
            "--fuzz-dir",
        ])
        .arg(root.join("fuzz"))
        .arg("--target-dir")
        .arg(&build)
        .arg(target)
        .arg(corpus)
        .arg("--")
        .arg(format!("-max_total_time={seconds}"))
        // The per-input timeout is a hang guard derived from the requested
        // run duration, never a new speed threshold for production parsing.
        .arg(format!("-timeout={seconds}"))
        // One outer OSC buffer reaches truncation and large/chunked APC cases.
        .arg(format!("-max_len={MAX_INPUT_BYTES}"))
        .arg("-print_final_stats=1")
        .arg(format!("-artifact_prefix={}/", artifacts.display()))
        .env("CARGO_TARGET_DIR", build)
        .current_dir(root);
    command
}

/// Copies only missing checked-in seeds; evolved coverage inputs are never erased.
fn seed(root: &Path, target: &str) -> Result<PathBuf, String> {
    // Only the mutable copies evolve. Repository seeds remain reviewable,
    // stable inputs derived from existing tests, rather than machine-local output.
    let corpus = root.join("target/fuzz/corpus").join(target);
    fs::create_dir_all(&corpus).map_err(|error| error.to_string())?;
    for entry in
        fs::read_dir(root.join("fuzz/seeds").join(target)).map_err(|error| error.to_string())?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            continue;
        }
        // A subsequent run resumes its existing corpus. Seeds are added by
        // filename only when absent, preserving coverage and diagnostic history.
        let destination = corpus.join(entry.file_name());
        if !destination.exists() {
            fs::copy(entry.path(), destination).map_err(|error| error.to_string())?;
        }
    }
    Ok(corpus)
}

/// Unit suffixes are explicit and overflow/zero cannot accidentally mean forever.
fn seconds(value: &str) -> Result<u64, String> {
    // These multipliers describe conventional time units, not parse budgets.
    // Keeping them here avoids introducing a duration-parsing dependency.
    let (number, multiplier) = match value.as_bytes().last() {
        Some(b's') => (&value[..value.len() - 1], 1),
        Some(b'm') => (&value[..value.len() - 1], 60),
        Some(b'h') => (&value[..value.len() - 1], 3600),
        _ => (value, 1),
    };
    // Reject signs and empty input before integer conversion; CLI arguments are untrusted.
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("invalid fuzz duration".into());
    }
    number
        .parse::<u64>()
        .ok()
        .and_then(|value| value.checked_mul(multiplier))
        .filter(|value| *value > 0)
        .ok_or("invalid fuzz duration".into())
}

#[cfg(test)]
/// Behavioral fixtures remain separate from the production command/manifest scanner.
mod tests;
