//! Strict export/header/consumer comparison, including whole-file API extensions.
// A top-level export file may be omitted with its independent protocol component.
// The catalogue still demands exact equality for every file that remains.
// Header fragments are followed from puzzle.h, so unused headers cannot claim declarations.
// Fixture fragments are followed from host.c, so unused tests cannot claim coverage.
// Neither feature unification nor a stale compiled archive supplies identifiers here.
// Linking the native caller is a separate proof of the actual C/Rust signatures.
use super::*;
/// Collects function identifiers without depending on a C/Rust parser crate.
///
/// Rust definitions must be public C exports, not incidental invocation text.
/// C input has its return type removed first, leaving the declaration identifier.
/// The closing test requires `(` immediately after the name, excluding type aliases.
/// This source check complements the actual link: the caller references every name.
pub(super) fn names(text: &str, rust: bool) -> BTreeSet<String> {
    // Declarations and definitions both place the identifier immediately before
    // `(`. Restrict Rust to public extern declarations, excluding test calls.
    text.lines()
        .filter(|line| !rust || line.contains("pub ") && line.contains("extern \"C\" fn"))
        .filter(|line| rust || line.trim_start().starts_with("puzzle_"))
        .filter_map(|line| {
            let start = line.find("puzzle_")?;
            let name: String = line[start..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            // A type name in a return value or a comment is not an exported function.
            line[start + name.len()..]
                .trim_start()
                .starts_with('(')
                .then_some(name)
        })
        .collect()
}
/// Verifies both directions of the export/header set, plus the C caller's coverage.
///
/// An omitted declaration and an unimplemented declaration are different failures.
/// Checking set equality catches both before a native executable is constructed.
/// Requiring a C invocation prevents newly exported operations escaping that witness.
/// Rust tests still validate malformed arguments independently of successful C calls.
pub(super) fn catalogue() -> Result<usize, String> {
    // Only the ABI crate defines exports; embedding internals are deliberately excluded.
    let root = "crates/puzzle-ffi";
    let header = included(
        &std::path::Path::new(root).join("include/puzzle.h"),
        "h",
        &mut BTreeSet::new(),
    )?;
    // Find the function name after the return type: C declarations start with
    // puzzle_result_t, so inspecting only the first puzzle_ token would miss it.
    let declared = names(&header.replace("puzzle_result_t ", ""), false);
    // Set comparison ignores definition order while rejecting duplicate catalogue drift.
    let mut exported = BTreeSet::new();
    for entry in fs::read_dir(format!("{root}/src")).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            exported.extend(names(
                &fs::read_to_string(path).map_err(|e| e.to_string())?,
                true,
            ));
        }
    }
    if declared != exported || exported.is_empty() {
        return Err(format!(
            "header-only: {:?}; export-only: {:?}",
            declared.difference(&exported).collect::<Vec<_>>(),
            exported.difference(&declared).collect::<Vec<_>>()
        ));
    }
    // The linked C executable is the independent consumer of this declaration set.
    let caller = included(
        &std::path::Path::new(root).join("tests/host.c"),
        "inc",
        &mut BTreeSet::new(),
    )?;
    for name in &exported {
        if !caller.contains(&format!("{name}(")) {
            return Err(format!("C test never invokes {name}"));
        }
    }
    Ok(exported.len())
}
/// Reads exactly the local fragments included by one translation unit.
/// A visited set rejects recursive includes rather than following a cycle forever.
fn included(
    path: &std::path::Path,
    extension: &str,
    visited: &mut BTreeSet<std::path::PathBuf>,
) -> Result<String, String> {
    // A duplicate include could hide an accidental recursive API fragment.
    if !visited.insert(path.to_path_buf()) {
        return Err(format!("recursive include: {}", path.display()));
    }
    // Read the same on-disk bytes that the strict C compiler consumes.
    let mut source =
        fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    // Ignore system includes; only quoted project fragments belong to this catalogue.
    let children: Vec<_> = source
        .lines()
        .filter_map(|line| line.trim().strip_prefix("#include \""))
        .filter_map(|line| line.strip_suffix('"'))
        .filter(|name| {
            std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext == extension)
        })
        .map(str::to_owned)
        .collect();
    // Resolve children relative to the including file, not the process cwd.
    for child in children {
        let child = path.parent().ok_or("include has no parent")?.join(child);
        // Appending preserves all identifiers without treating unreferenced files as evidence.
        source.push_str(&included(&child, extension, visited)?);
    }
    Ok(source)
}
