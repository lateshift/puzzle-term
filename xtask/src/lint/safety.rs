//! R8/D-53 exception boundaries and auditable foreign-call safety comments.
//!
//! Clippy checks unsafe syntax and undocumented blocks; this additional gate
//! prevents an exception crate's dependency or code from leaking into the core.
//! Safety contracts must be the adjacent comment paragraph above every block.
//! Target tables matter even when Cargo would simply omit a dependency elsewhere:
//! listing CoreVideo unconditionally is an architectural error, not a compile test.
//! File paths are inspected by components so a similarly named directory cannot
//! inherit the exception accidentally. Ordinary crates retain the unconditional
//! unsafe-token ban in the caller; this module never broadens that existing rule.
use super::comments::scan_line;
use std::path::Path;

/// Only the two user-approved foreign-interface crates may contain unsafe tokens.
pub(super) fn exception(path: &Path) -> bool {
    path.components()
        .any(|part| matches!(part.as_os_str().to_str(), Some("puzzle-sys" | "puzzle-ffi")))
}

/// Requires the approved native display/presentation family in a macOS target table.
/// Runtime classes, blocks, IOSurface, Metal and CoreFoundation remain one boundary:
/// allowing a wrapper crate is not permission to import its bindings into the core.
/// Harness-only AppKit clipboard bindings retain their separate existing rule.
pub(super) fn manifest(path: &Path, data: &str) -> Result<(), String> {
    // Cargo manifests sit directly inside their owning crate directory.
    // A target table cannot authorize another crate to use this OS API.
    let sys = path
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|owner| owner == "puzzle-sys");
    let mut table = "";
    for line in data.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            // Preserve the complete header, distinguishing macOS from any-target
            // dependencies and from a similarly spelled non-macOS condition.
            table = line;
        // A target table is required even when the dependency uses workspace inheritance.
        } else if (line.starts_with("objc2 =")
            || line.starts_with("objc2.")
            || line.starts_with("block2")
            || line.starts_with("objc2-metal")
            || line.starts_with("objc2-io-surface")
            || line.starts_with("objc2-core-foundation")
            || line.starts_with("objc2-core-video")
            || line.starts_with("objc2-quartz-core")
            || line.starts_with("objc2-core-graphics")
            || (sys && line.starts_with("wgpu")))
            && (!sys || table != "[target.'cfg(target_os = \"macos\")'.dependencies]")
        {
            return Err(format!(
                "{}: native display/layer bindings belong only in puzzle-sys's macOS dependencies",
                path.display()
            ));
        }
    }
    Ok(())
}

/// Checks contracts without counting tokens inside comments or Rust strings.
pub(super) fn source(path: &Path, data: &str) -> Result<(), String> {
    // Nonexception files are handled by check_tokens' original unsafe ban.
    if !exception(path) {
        return Ok(());
    }
    let lines: Vec<_> = data.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let (active, _) = scan_line(line);
        // Removing comments and strings first avoids recognizing documentation,
        // diagnostic messages or the scanner's own spelling as an unsafe block.
        for suffix in active.split("unsafe").skip(1) {
            // Attributes and function declarations are checked by Rust/Clippy;
            // actual block expressions require the adjacent contract paragraph.
            let suffix = suffix.trim_start();
            // A newline before the opening brace still belongs to this expression.
            let block = suffix.starts_with('{')
                || (suffix.is_empty()
                    && lines
                        .get(index + 1)
                        .is_some_and(|line| line.trim_start().starts_with('{')));
            if block && !contract(&lines[..index]) {
                // Each expression is checked independently: the previous block's
                // valid contract must not authorize a later undocumented call.
                return Err(format!(
                    "{}:{}: unsafe block needs an adjacent // SAFETY: contract",
                    path.display(),
                    index + 1
                ));
            }
        }
    }
    Ok(())
}

/// Multiline safety prose stays adjacent; unrelated earlier comments do not count.
fn contract(before: &[&str]) -> bool {
    // Rustfmt may split `let status = unsafe { ... }` after the assignment;
    // its safety paragraph remains attached to that same statement.
    let before = if before
        .last()
        .is_some_and(|line| line.trim().starts_with("let ") && line.trim().ends_with('='))
    {
        &before[..before.len() - 1]
    } else {
        before
    };
    before
        .iter()
        .rev()
        .take_while(|line| line.trim_start().starts_with("//"))
        .any(|line| line.trim_start().starts_with("// SAFETY:"))
}

#[cfg(test)]
mod tests;
