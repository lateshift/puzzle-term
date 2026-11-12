//! D-50 ownership gates inspect executable tokens, never examples in comments.
//! Shared Session guards are forbidden even in tests. Live window/render modules
//! may use only channel endpoints; startup may construct a Session before moving
//! it into its worker. Synchronous diagnostic requests stay in explicitly scoped
//! harness/probe modules and tests, keeping the normal input path nonblocking.
//!
//! A successful build alone cannot protect this rule: a Mutex around Session
//! is legal Rust, as is calling a synchronous diagnostic helper from a key handler.
//! The scanner therefore checks executable spellings before the ordinary lints.
//! String and comment masking lets evidence describe the old design without
//! turning that description into a forbidden implementation.
//!
//! A shared guard can be nested inside Arc or another container. Guard argument
//! brackets are balanced before inspecting the complete enclosed type, so the
//! ownership gate applies to Mutex<Session>, RwLock<Session> and nested wrappers.
//! Import aliases and local type aliases are followed to the same Session name.
//! This adds no source-parser dependency; Cargo still verifies Rust syntax.
//!
//! Startup constructs VT before transferring it into the owner thread. That
//! constructor scope is allowed, while ordinary live window and render modules
//! may retain only endpoint types and immutable publication values. Unit tests
//! may construct an owner fixture, but may not restore a shared Session guard.
//! Diagnostic probe modules are separately named production scopes because their
//! feature-enabled closures intentionally execute on the Session owner. Their
//! synchronous replies are never called from normal keyboard or pointer modules.
//!
//! The neutral VT and Session crates cannot import threading primitives at all.
//! Test observers may use synchronization for independent write logs and counters;
//! this exemption grants no shared guard around terminal state. GPU device-loss
//! callbacks and platform channel internals are outside those two pure owners.
use super::comments::scan_line;
use std::{fs, path::Path};

#[cfg(test)]
/// Violations reproduce ownership regressions without adding runtime machinery.
mod tests;

/// Traverses owned crate sources for the same structural acceptance gate as lint.
pub(crate) fn check() -> Result<(), String> {
    // The unrelated TypeScript project and build artifacts cannot supply source here.
    walk(Path::new("crates"))
}

/// Source traversal includes unit tests so a legacy shared guard cannot survive there.
fn walk(directory: &Path) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        // Recursive paths preserve the module identity used by the live/probe policy.
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            walk(&path)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            // Read every owned Rust file, including fixtures that compile only in tests.
            let data = fs::read_to_string(&path).map_err(|error| error.to_string())?;
            source(&path, &data)?;
        }
    }
    Ok(())
}

/// Imports and complete type-alias right sides cannot conceal a terminal or guard.
fn aliases(code: &str, initial: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = initial.iter().map(|name| (*name).to_owned()).collect();
    loop {
        // Transitive aliases form a finite graph; deduplication ends the traversal.
        let before = names.len();
        // Semicolon boundaries preserve complete qualified/generic type-alias right sides.
        for statement in code.split(';') {
            let words: Vec<_> = statement
                .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
                .filter(|word| !word.is_empty())
                .collect();
            for triple in words.windows(3) {
                // Qualified imports still place their final identifier before `as`.
                if names.iter().any(|name| name == triple[0])
                    && triple[1] == "as"
                    && !names.iter().any(|name| name == triple[2])
                {
                    names.push(triple[2].to_owned());
                }
            }
            if let Some(index) = words.iter().position(|word| *word == "type")
                && let Some(alias) = words.get(index + 1)
                && words[index + 2..]
                    .iter()
                    .any(|word| names.iter().any(|name| name == word))
                && !names.iter().any(|name| name == alias)
            {
                // Inspect the complete right side, including qualified or wrapped types.
                names.push((*alias).to_owned());
            }
        }
        // Cyclic alias edges cannot cause an unbounded lint loop.
        if names.len() == before {
            return names;
        }
    }
}

/// Checks guard type arguments with balanced brackets, including qualified names.
fn shared_guard(code: &str, compact: &str, names: &[String]) -> bool {
    // Guard import aliases are subject to the same rule as their canonical names.
    // The compact view lets a type argument start on the next physical source line.
    for guard in aliases(code, &["Mutex", "RwLock"]) {
        for (_, suffix) in compact
            .match_indices(&guard)
            .map(|(i, _)| (i, &compact[i + guard.len()..]))
        {
            // A method or variable containing the guard name is not a generic type.
            let Some(suffix) = suffix.strip_prefix('<') else {
                continue;
            };
            // The opening bracket was consumed; stop only at its matching closing bracket.
            let mut depth = 1;
            let end = suffix
                .find(|character| {
                    match character {
                        '<' => depth += 1,
                        '>' => depth -= 1,
                        _ => {}
                    }
                    depth == 0
                })
                .unwrap_or(suffix.len());
            // Qualified and nested type names remain visible inside the outer guard.
            if suffix[..end]
                .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
                .any(|word| names.iter().any(|name| name == word))
            {
                return true;
            }
        }
    }
    false
}

/// Applies the platform ownership rules to one source after masking strings/comments.
fn source(path: &Path, data: &str) -> Result<(), String> {
    // The existing lexical mask removes comments and quoted diagnostic examples.
    let code = data
        .lines()
        .map(|line| scan_line(line).0)
        .collect::<Vec<_>>()
        .join("\n");
    // Whitespace across lines cannot disguise a guard or diagnostic method call.
    let compact: String = code.chars().filter(|ch| !ch.is_whitespace()).collect();
    // Closed identifier comparisons avoid confusing FrameSnapshot with Session.
    let words: Vec<_> = code
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .filter(|word| !word.is_empty())
        .collect();
    // Resolve aliases before checking wrappers, so imported short names cannot bypass D-50.
    let names = aliases(&code, &["Session", "SharedSession"]);
    // Legacy wrapper names are forbidden even before their fields acquire a lock.
    if shared_guard(&code, &compact, &names) || words.contains(&"SharedSession") {
        return Err(format!(
            "{}: D-50 forbids a shared Session guard",
            path.display()
        ));
    }
    // Reachability checks use module roles after the universal guard prohibition.
    host_paths(path, &compact, &words)
}

/// Live host scopes accept endpoints; the two core ownership crates accept no locks.
fn host_paths(path: &Path, compact: &str, words: &[&str]) -> Result<(), String> {
    let name = path.to_string_lossy();
    // Tests can build independent owners but cannot borrow the live worker.
    let test = path.components().any(|part| part.as_os_str() == "tests")
        || path.file_name().is_some_and(|part| part == "tests.rs");
    let desktop = name.contains("puzzle-desktop/src/") || name.contains("puzzle-embed/src/");
    // Only these named feature/harness scopes may issue owner-side closure diagnostics.
    let probe = name.contains("/harness/")
        || name.contains("/session_thread/")
        || name.ends_with("/session_thread.rs")
        || path.components().any(|part| {
            let part = part.as_os_str().to_string_lossy();
            part.strip_suffix(".rs")
                .unwrap_or(&part)
                .ends_with("_probe")
                || part == "glyph_bisect.rs"
        });
    // A live helper cannot regain synchronous queries by hiding behind the endpoint type.
    if desktop
        && !test
        && !probe
        && (compact.contains(".probe(") || compact.contains("Control::Probe"))
    {
        return Err(format!(
            "{}: D-50 Probe outside diagnostic modules",
            path.display()
        ));
    }
    // Startup is a transfer boundary, not a long-lived synchronous Session owner.
    let live = desktop
        && (name.contains("/window/")
            || name.contains("/render_thread/")
            || name.ends_with("/render_thread.rs")
            || name.ends_with("/window.rs"));
    if live && !test && !probe && !name.ends_with("/window/start.rs") && words.contains(&"Session")
    {
        return Err(format!(
            "{}: D-50 live host cannot reach Session",
            path.display()
        ));
    }
    core_sync(path, words, test)
}

/// Platform schedulers may synchronize; the VT and Session implementations may not.
fn core_sync(path: &Path, words: &[&str], test: bool) -> Result<(), String> {
    let name = path.to_string_lossy();
    // Test-only clock/write-log observers do not become dependencies of the shipped core.
    if !test
        && (name.contains("puzzle-term/") || name.contains("puzzle-vt/"))
        && words.iter().any(|word| {
            matches!(*word, "Mutex" | "RwLock" | "Condvar") || word.starts_with("Atomic")
        })
    {
        return Err(format!(
            "{}: D-50 core must remain free of locks and atomics",
            path.display()
        ));
    }
    Ok(())
}
