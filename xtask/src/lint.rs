//! Mechanical repository checks required by design §2.4.
//!
//! These checks scan only workspace-owned Rust, WGSL, and manifests. The
//! unrelated puzzle/ tree is never opened. Cargo and Clippy verify syntax;
//! this scanner verifies project rules they cannot know about.
//!
//! The walker starts at `crates/`, `xtask/` and isolated `fuzz/`, so build output
//! and the unrelated TypeScript project cannot affect Rust acceptance. It
//! applies manifest checks to every owned Cargo manifest and source checks to
//! every Rust or WGSL file. The comment guard is shared across this traversal:
//! repetitions in two different crates count toward the same threshold.
//!
//! Cargo already parses manifests and resolves versions. This scanner adds a
//! direct-dependency gate tied to the approved list in design §2.6 and checks
//! the internal dependency edges from design §4.1. Its manifest scan only needs
//! the dependency tables used in M0; it deliberately has no TOML dependency.
//!
//! Source checks begin with the module header, then count comment-bearing
//! lines using a lexer that knows when a slash pair is inside a Rust string.
//! A comment can sit beside code and count as both a code and comment line.
//! The forty-percent ratio is a floor; the separate quality check rejects
//! repeated explanations and generic prose even when the ratio is met.
//!
//! Function-size counting is a lightweight physical-line scan. Clippy's
//! `too_many_lines` lint is the second gate for real Rust functions. WGSL
//! shaders are scanned for headers and source size; their mathematical
//! correctness will be established by the M3 CPU/GPU oracle.
//!
//! Platform dependency checks distinguish browser APIs from native window
//! and PTY APIs before compilation: a dependency that happens to compile on
//! another target is still an architectural violation. The neutral renderer
//! may use wgpu, but its source cannot refer to browser or winit handles.
//! `TODO(TASK: …)` comments must name text in the committed task registry;
//! merely using the marker's syntax does not make unfinished work tracked.
//! Generated settings artifacts are absent in M0 because their source
//! generators arrive with the settings subsystem in M6. The check makes an
//! accidental early artifact fail until that generator is implemented.
//! M18's approved clipboard dependencies remain confined to native host crates.
//! getrandom belongs to embed for secure paste capabilities and to the PTY bridge
//! for its existing connection token; neither dependency enters shared core crates.
//! NSPasteboard bindings belong only to the macOS harness for revision tracking.
//! arboard's image support remains harness-only rather than leaking into embedding.
//! The C interface transports MIME bytes without linking a clipboard codec.
//! The wasm dependency-tree check continues to reject native-only libraries.
//! A matching allowlist name alone does not waive these crate ownership boundaries.
//! Approved dependency placement is checked independently of version resolution.
//! Manifest versions still inherit the workspace catalogue for reproducible builds.
//! Clipboard additions do not expand the puzzle-sys unsafe exception.
//! Every FFI unsafe range access retains its adjacent SAFETY contract.
//! The platform-neutral protocol parser uses no clock, thread or OS randomness.
//! Native and browser clipboard policies share one Session-owned implementation.
/// Rejects misplaced artifacts before source walkers can inspect generated files.
mod build_dirs;
mod comments;
/// M17 keeps native host callbacks on App/Surface instead of diagnostic owners.
mod embedding;
/// Checks the resolved browser dependency tree for GPU backend drift.
mod gpu_tree;
/// D-53 confines foreign display-link code and its dependency to puzzle-sys.
mod safety;
/// D-50 forbids shared Session access and live diagnostic backdoors.
pub(crate) mod session;

use self::comments::{QualityGuard, scan_line};
use std::{collections::BTreeSet, fs, path::Path};
/// The isolated fuzz workspace owns only its approved local version catalogue.
mod fuzz;
/// Applies all M0 source and manifest policies, returning the first violation.
///
/// Failure order follows directory traversal so diagnostics identify a
/// concrete file. The `QualityGuard` lives for the whole run, which catches
/// a copied comment even when each individual file looks well documented.
pub(crate) fn run() -> Result<(), String> {
    // Tests also write evidence: Cargo's target setting cannot relocate their relative paths.
    build_dirs::check(Path::new("crates"))?;
    session::check()?;
    embedding::check()?;
    // The committed allowlist makes dependency changes visible in review.
    let allowed = fs::read_to_string("xtask/deps-allowlist.txt")
        // The allowlist is committed, so approvals are visible in Git review.
        .map_err(|error| error.to_string())?
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    // The unrelated TypeScript tree is outside this rule scanner.
    let mut quality = QualityGuard::default();
    for root in ["crates", "xtask", "fuzz"] {
        if Path::new(root).exists() {
            walk(Path::new(root), &allowed, &mut quality)?;
        }
    }
    crate::bus_docs::check_catalogue()?;
    crate::bus_docs::check_generated()?;
    gpu_tree::check()?;
    check_generated()
}
/// Descends only into the owned source directories.
///
/// Directories are visited recursively because later milestones add sibling
/// modules in the `foo.rs` plus `foo/` style prescribed by design §2.2.
fn walk(dir: &Path, allowed: &BTreeSet<String>, quality: &mut QualityGuard) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|error| error.to_string())? {
        // Each manifest and source file is visited exactly once by this walk.
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            walk(&path, allowed, quality)?;
        } else if path.file_name().is_some_and(|name| name == "Cargo.toml") {
            manifest(&path, allowed)?;
        } else if path
            .extension()
            .is_some_and(|ext| ext == "rs" || ext == "wgsl")
        {
            source(&path, quality)?;
        }
    }
    Ok(())
}
/// Checks direct crate names, inherited versions, and permitted internal edges.
///
/// A crate may name an external dependency only if the approved text file
/// contains it, and the manifest must inherit its version from the workspace.
/// Internal `puzzle-*` edges use a separate table because their local path is
/// not a crates.io version. This prevents an upper layer from reaching around
/// `puzzle-term` or importing a platform shell from a neutral crate.
fn manifest(path: &Path, allowed: &BTreeSet<String>) -> Result<(), String> {
    let data = fs::read_to_string(path).map_err(|error| error.to_string())?;
    safety::manifest(path, &data)?;
    let fuzz = fuzz::manifest(path, &data)?;
    let owner = path
        // The manifest's parent directory is its crate name in this layout.
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .ok_or("bad manifest path")?;
    let mut dependencies = false;
    // Target-specific dependency tables still contain the word dependencies.
    for line in data.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            // Catalogue entries define versions; actual fuzz dependencies below
            // still undergo every ordinary inheritance/allowlist/edge check.
            dependencies = trimmed.contains("dependencies")
                && !(fuzz && trimmed == "[workspace.dependencies]");
            continue;
        }
        if !dependencies || trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        let name = name
            // Cargo's shorthand `wgpu.workspace = true` has the same meaning.
            .trim()
            .strip_suffix(".workspace")
            .unwrap_or(name.trim());
        if name.starts_with("puzzle-") {
            // Internal edges are stricter than the external allowlist.
            let permitted = internal_edges(owner).contains(&name);
            if !permitted {
                return Err(format!(
                    "{}: forbidden internal edge {owner} → {name}",
                    path.display()
                ));
            }
            continue;
        }
        external_dependency(path, owner, name, value, trimmed, allowed)?;
    }
    Ok(())
}

/// External crate approval, platform ownership and inherited versions are one contract.
fn external_dependency(
    path: &Path,
    owner: &str,
    name: &str,
    value: &str,
    trimmed: &str,
    allowed: &BTreeSet<String>,
) -> Result<(), String> {
    // Manifest traversal already removed comments and normalized Cargo shorthand;
    // these checks apply equally to ordinary and target-specific dependency tables.
    if !allowed.contains(name) {
        return Err(format!("{}: unapproved dependency {name}", path.display()));
    }
    // Approval of a version never authorizes importing its OS API into the core.
    if !platform_dependency_allowed(owner, name) {
        return Err(format!(
            "{}: {name} crosses a platform boundary",
            path.display()
        ));
    }
    if !value.contains("workspace = true")
        // A literal version here would bypass the root version decision.
        && !value.contains("workspace=true")
        && !trimmed.contains(".workspace = true")
    {
        return Err(format!(
            "{}: {name} must inherit its workspace version",
            path.display()
        ));
    }
    Ok(())
}

/// Restricts target APIs to the shells named by project rule R9.
/// Browser glue belongs in `puzzle-web`; winit and PTY facilities stay in
/// native shells. The shared wgpu API is permitted at the renderer seam.
fn platform_dependency_allowed(owner: &str, name: &str) -> bool {
    if let Some(allowed) = extra_dependency(owner, name) {
        return allowed;
    }
    match name {
        // M19.2b approval is only for the isolated, unpublished fuzz workspace.
        "libfuzzer-sys" | "arbitrary" => owner == "fuzz",
        "wasm-bindgen"
        | "wasm-bindgen-futures"
        | "web-sys"
        | "js-sys"
        | "tracing-web"
        | "console_error_panic_hook" => owner == "puzzle-web",
        "winit" => owner == "puzzle-desktop",
        // R4 approvals: safe PTY duplication and native image inflation stay in embed.
        "filedescriptor" | "flate2" => owner == "puzzle-embed",
        "png" | "fontique" | "rustix" => matches!(owner, "puzzle-desktop" | "puzzle-embed"),
        // R8 scopes both foreign facilities to the same audited platform glue crate.
        "objc2"
        | "block2"
        | "objc2-metal"
        | "objc2-io-surface"
        | "objc2-core-foundation"
        | "objc2-core-video"
        | "objc2-quartz-core"
        | "objc2-core-graphics" => owner == "puzzle-sys",
        "portable-pty" => matches!(
            owner,
            "puzzle-desktop" | "puzzle-embed" | "puzzle-pty-bridge"
        ),
        "getrandom" => matches!(owner, "puzzle-pty-bridge" | "puzzle-embed"),
        "objc2-app-kit" | "arboard" => owner == "puzzle-desktop",
        "tokio" | "tokio-tungstenite" | "futures-util" => owner == "puzzle-pty-bridge",
        // D-54 sys borrows the macOS HAL solely for safe layer-property glue.
        "wgpu" => matches!(
            owner,
            "puzzle-render"
                | "puzzle-web"
                | "puzzle-desktop"
                | "puzzle-embed"
                | "puzzle-ffi"
                | "puzzle-sys"
        ),
        _ => true,
    }
}

/// Verifies the three settings artifacts against the typed schema source.
fn check_generated() -> Result<(), String> {
    crate::settings_docs::check()
}
/// Gives the exact internal edges in design §4.1, including M0's renderer seam.
///
/// Most skeleton crates have no internal dependencies yet. This full graph
/// describes the allowed future direction without adding M1 implementation.
fn internal_edges(owner: &str) -> Vec<&'static str> {
    // The catalogue is data, so distribution profiles can remove entire optional edges.
    include_str!("../data/dependency-edges.txt")
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| *name == owner)
        .map_or_else(Vec::new, |(_, names)| names.split_whitespace().collect())
}
// Additional dependency ownership belongs beside its optional implementation.
include!("lint/optional_items.rs");
/// Checks comment coverage, size, forbidden constructs, and platform boundaries.
///
/// Tests are exempt from the density floor because their cases should stay
/// concise, though the quality guard still inspects their actual comments.
/// The 400-line threshold reports a warning for early splitting; 500 lines
/// is the hard failure mandated by R7. The path is then passed to independent
/// function-size and token checks after the comment gate succeeds.
fn source(path: &Path, quality: &mut QualityGuard) -> Result<(), String> {
    let data = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let lines: Vec<&str> = data.lines().collect();
    let is_test = is_test_file(path);
    // Tests have different comment-density expectations but the same size cap.
    if lines.len() > 500 {
        return Err(format!(
            "{}: {} lines exceeds 500",
            path.display(),
            lines.len()
        ));
    }
    if lines.len() > 400 {
        eprintln!("{}: warning: over 400 lines", path.display());
    }
    let first = lines.first().copied().unwrap_or("");
    // A module explains its purpose before any imports or implementation.
    if !first.starts_with("//!") && !first.starts_with("//") {
        return Err(format!("{}: missing module header", path.display()));
    }
    let mut comments = 0;
    let mut code = 0;
    for line in &lines {
        // The lexer ignores slash pairs inside diagnostic strings and URLs.
        let (active, comment) = scan_line(line);
        if !active.trim().is_empty() {
            code += 1;
        }
        if let Some(text) = comment {
            comments += 1;
            quality.observe(path, text)?;
        }
    }
    if !is_test && comments * 5 < code * 2 {
        // Multiplication avoids floating-point rounding at the exact boundary.
        return Err(format!(
            "{}: comment density {comments}/{code} below 40%",
            path.display()
        ));
    }
    check_functions(path, &lines)?;
    check_tokens(path, &data)?;
    Ok(())
}
/// Counts each function's physical lines without a Rust parser dependency.
///
/// This is intentionally conservative for ordinary `fn` and `async fn`
/// declarations in workspace-owned source. Clippy's Rust parser provides the
/// authoritative second check for bodies that use more complex syntax.
/// Braces track the physical span, so comments inside a body count toward R7.
fn check_functions(path: &Path, lines: &[&str]) -> Result<(), String> {
    let mut start: Option<usize> = None;
    let mut depth = 0_i32;
    // A declaration can place its opening brace on a later line.
    let mut opened = false;
    for (index, line) in lines.iter().enumerate() {
        // Only the start of a function opens a new line-count window.
        if start.is_none()
            && (line.trim_start().starts_with("fn ")
                || line.trim_start().starts_with("pub(crate) fn ")
                || line.trim_start().starts_with("pub async fn ")
                || line.trim_start().starts_with("async fn "))
        {
            start = Some(index);
            opened = false;
            depth = 0;
        }
        if let Some(begin) = start {
            // Nested blocks keep the function open until their braces close.
            for ch in line.chars() {
                if ch == '{' {
                    depth += 1;
                    opened = true;
                } else if ch == '}' {
                    depth -= 1;
                }
            }
            if opened && depth <= 0 {
                // Report the declaration line so the file can be split by role.
                if index - begin + 1 > 60 {
                    return Err(format!(
                        "{}:{}: function exceeds 60 lines",
                        path.display(),
                        begin + 1
                    ));
                }
                start = None;
            }
        }
    }
    Ok(())
}
/// Rejects forbidden APIs, placeholders, and cross-platform imports.
///
/// The lexical scan removes string contents and comments before comparing
/// active code with restricted API names. This allows the lint source itself
/// to mention a forbidden spelling in a diagnostic without an exemption.
/// Test source alone may use `unwrap` and `expect` as specified by R8.
/// A placeholder is legal only with a progress entry reference, so deferred
/// work cannot silently become an implementation stub.
/// Reading the progress log only for a found marker keeps ordinary files
/// cheap to scan while still proving that a deferred task was recorded.
fn check_tokens(path: &Path, data: &str) -> Result<(), String> {
    safety::source(path, data)?;
    let name = path.to_string_lossy();
    let web = name.contains("puzzle-web/");
    let desktop = name.contains("puzzle-desktop/");
    let bridge = name.contains("puzzle-pty-bridge/");
    // Native engine imports move out of the toolkit host under M17.embed.
    let embed = name.contains("puzzle-embed/");
    let binary = desktop || name.contains("xtask/") || (bridge && name.ends_with("src/main.rs"));
    // The desktop and xtask binaries may print CLI diagnostics under R10.
    let is_test = is_test_file(path);
    for line in data.lines() {
        // Compare only executable tokens; comments are checked separately.
        let (active, comment) = scan_line(line);
        for (token, allowed) in [
            ("cfg(target_arch", web),
            ("wasm_bindgen", web),
            ("web_sys", web),
            ("js_sys", web),
            ("winit::", desktop),
            ("portable_pty", desktop || bridge || embed),
            ("println!", binary),
            ("eprintln!", binary),
            ("dbg!", false),
            ("console::", false),
            ("unsafe", safety::exception(path)),
            ("unwrap()", is_test),
            ("expect(", is_test),
        ] {
            if !allowed && active.contains(token) {
                return Err(format!("{}: forbidden token {token}", path.display()));
            }
        }
        if let Some(text) = comment {
            // Untracked placeholders would leave work outside README.md.
            if text.contains("TODO") || text.contains("FIXME") {
                let entry = text
                    .split_once("TODO(TASK:")
                    .and_then(|(_, rest)| rest.split_once(')'))
                    .map(|(entry, _)| entry.trim())
                    .filter(|entry| !entry.is_empty())
                    .ok_or_else(|| format!("{}: untracked placeholder", path.display()))?;
                let progress =
                    fs::read_to_string("xtask/data/tasks.md").map_err(|error| error.to_string())?;
                if !progress.contains(entry) {
                    return Err(format!("{}: missing tracked task: {entry}", path.display()));
                }
            }
        }
    }
    Ok(())
}

/// Recognizes both `foo/tests.rs` unit tests and integration test directories.
fn is_test_file(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "tests.rs")
        || path.components().any(|part| part.as_os_str() == "tests")
}
