//! Runs the documented scenario regression interface without shell globbing.
//! Each scenario keeps its embedded assertions; --check also compares every
//! committed image or grid reference using the existing golden tolerance.
//!
//! Discovery does not modify assets or depend on shell glob expansion. Sorting
//! gives stable execution order on every filesystem. Each child owns a separate
//! native renderer, so live-PTY scenes cannot leak terminal state into the next.
//! Failures accumulate across scripts to keep the complete regression visible.
//!
//! References belong to the directory beside the script, matching the harness's
//! output stem. Missing generated files fail the comparator; unreferenced new
//! images remain review candidates. Text files are compared exactly because a
//! changed cell or parser reply has no meaningful pixel tolerance.
//!
//! Only --check is removed from the selector list. Unexpected extra arguments
//! produce a usage error rather than leaving --check silently unprocessed.
//! Paths go directly into argv, so spaces or shell metacharacters stay literal.
//! All Cargo children inherit the workspace root and its common target directory.
//! The native scenario's own diagnostics and failed-pixel bundles remain under
//! target/harness, while this coordinator emits one result per executed script.
//! Comparing references never overwrites them: review and blessing remain the
//! existing separate golden workflow with its human approval requirement.
//! Discovery traverses directories but not symlinks; nested scripts participate
//! without allowing an external directory to change the regression inventory.
//! A missing scenario tree and an empty tree are errors, not successful no-ops.
//! Individual script failures do not prevent unrelated scenes from executing.
//! The final error preserves every failed script and its original diagnostic.
use std::{fs, path::Path};

/// Accepts a named script or discovers every checked-in scenario recursively.
pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let check = args.iter().any(|arg| arg == "--check");
    let inputs: Vec<_> = args.iter().filter(|arg| *arg != "--check").collect();
    let [input] = inputs.as_slice() else {
        return Err("usage: harness FILE|--all [--check]".into());
    };
    let mut paths = Vec::new();
    if input.as_str() == "--all" {
        discover(Path::new("assets/scenarios"), &mut paths)?;
    } else if input.starts_with('-') {
        return Err(format!("unknown harness option: {input}"));
    } else {
        paths.push(Path::new(input).to_path_buf());
    }
    paths.sort();
    if paths.is_empty() {
        return Err("no harness scripts found".into());
    }
    let mut failures = Vec::new();
    for path in paths {
        let result = scenario(&path, check);
        match result {
            Ok(()) => println!("PASS harness {}", path.display()),
            Err(error) => {
                println!("FAIL harness {}: {error}", path.display());
                failures.push(format!("{}: {error}", path.display()));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

/// Nested terminal-frame scripts are regressions too, not reference-only data.
fn discover(directory: &Path, paths: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        // Do not follow symlinks out of the authored scenario tree.
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            discover(&path, paths)?;
        } else if path.extension().is_some_and(|ext| ext == "pzs") {
            paths.push(path);
        }
    }
    Ok(())
}

/// Fresh execution precedes reference discovery, including scenes without goldens.
fn scenario(path: &Path, check: bool) -> Result<(), String> {
    let script = path.to_str().ok_or("non-UTF-8 scenario path")?;
    let source = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut args = vec!["run", "-p", "puzzle-desktop"];
    // Only authored diagnostic-grant scenes need the optional implementation.
    extra_features(&source, &mut args);
    args.extend(["--", "harness", script]);
    super::quiet("cargo", &args)?;
    if !check {
        return Ok(());
    }
    let stem = path.file_stem().ok_or("scenario stem missing")?;
    let references = path.with_extension("");
    if !references.is_dir() {
        // New review scenes still enforce their authored pixel/ink assertions.
        // Absence of a golden is not permission to bless one automatically (R15).
        return Ok(());
    }
    let output = Path::new("target/harness").join(stem);
    for entry in fs::read_dir(references).map_err(|e| e.to_string())? {
        let reference = entry.map_err(|e| e.to_string())?.path();
        let actual = output.join(reference.file_name().ok_or("reference name missing")?);
        if reference.extension().is_some_and(|ext| ext == "png") {
            crate::golden::compare(&actual, &reference)?;
        } else if reference.extension().is_some_and(|ext| ext == "txt") {
            // Grid dumps are exact text contracts; image tolerances cannot hide them.
            if fs::read(&actual).map_err(|e| e.to_string())?
                != fs::read(&reference).map_err(|e| e.to_string())?
            {
                return Err(format!("text reference differs: {}", reference.display()));
            }
        }
    }
    Ok(())
}

// Optional fixture compilation is separate from standard scenario execution.
include!("harness/optional_items.rs");
