//! Generates built-in theme constants from reviewed palette JSON (§14.6).
//!
//! The source files carry pinned upstream URLs, commits and license names.
//! Rust receives literal terminal colors only; missing UI tokens are mixed
//! through OKLab at runtime by `puzzle-term::theme::derive`. Regeneration is
//! deterministic and `check` refuses edits to constants without source edits.
//!
//! The approved assets are JSON so a reviewer can compare one foreground,
//! background or ANSI slot directly with the cited upstream file. The
//! generator intentionally has no importer for kitty, Xresources or a
//! user's arbitrary theme file; that work is deferred by D-25. Conversion
//! from the pinned upstream formats happened once during asset review.
//! From then on, JSON is the checked-in source of truth for compilation.
//!
//! `null` is meaningful for a terminal token the source did not define:
//! the neutral runtime derives it through the documented OKLab rule.
//! Writing a plausible literal into JSON would falsely attribute a Puzzle
//! choice to another palette author. The generated comments preserve this
//! distinction beside exact byte values in Rust's compile-time table.
//!
//! Four Puzzle entries and ten external entries are required. A missing
//! file must fail generation instead of shipping a shorter gallery with
//! a still-green build. The same count also guards accidental extra JSON
//! files in the theme directory, such as an editor's backup export.
//!
//! The generator uses two seven-theme shards because rustfmt expands a
//! `ThemeSeed` into several lines. This keeps the generated source within
//! R7's file limit while the root index preserves one stable ordering.
//! `#[rustfmt::skip]` keeps literal rows compact and makes the code-size
//! lint inspect provenance comments instead of hundreds of brace lines.
//!
//! Verification regenerates into ignored `target/` scratch files and
//! compares byte-for-byte with the committed generated files. It never
//! updates the tracked outputs during `cargo xtask check`; developers run
//! `cargo xtask themes` when changing a JSON palette intentionally.
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Generated root and two shards keep every source file under R7's limit.
const GENERATED: [&str; 3] = [
    "crates/puzzle-term/src/theme/generated.rs",
    "crates/puzzle-term/src/theme/generated/a.rs",
    "crates/puzzle-term/src/theme/generated/b.rs",
];
/// Reviewed palette documents, not user config or arbitrary JSON imports.
const SOURCES: &str = "assets/themes";

/// Loads a required string without silently accepting a changed JSON shape.
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    // An absent source URL or credit is an asset-review failure. Using an
    // empty fallback would let a generated palette lose its attribution.
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("theme missing string {key}"))
}

/// Converts one official `#rrggbb` value into literal byte components.
fn color(value: &Value, key: &str) -> Result<[u8; 3], String> {
    // Only six-digit literals are copied. Alpha belongs to derived UI
    // overlays, and parsing a named color would depend on host CSS rules.
    let text = string(value, key)?;
    let hex = text
        .strip_prefix('#')
        .filter(|digits| digits.len() == 6)
        .ok_or_else(|| format!("{key} must be #rrggbb"))?;
    let mut rgb = [0; 3];
    // The byte order matches `#rrggbb`; shifting or little-endian packing
    // here would tint every built-in even while the JSON looked correct.
    for (index, channel) in rgb.iter_mut().enumerate() {
        let start = index * 2;
        *channel = u8::from_str_radix(&hex[start..start + 2], 16)
            .map_err(|error| format!("{key}: {error}"))?;
    }
    Ok(rgb)
}

/// Emits a stable Rust array expression for one exact source color.
fn rgb(color: [u8; 3]) -> String {
    // Decimal bytes are emitted as a Rust array, not a packed integer, so
    // the renderer can use them without endian-specific reinterpretation.
    format!("[{},{},{}]", color[0], color[1], color[2])
}

/// Restricts metadata to the licenses approved for the D-25 pack.
fn checked_metadata(value: &Value, path: &Path) -> Result<(), String> {
    // D-25 authorizes only MIT or Apache-2.0 source material. The JSON
    // license is verified against a pinned upstream license or file header
    // during vendoring, then checked here against the approved set.
    let license = string(value, "license")?;
    if !matches!(license, "MIT" | "Apache-2.0") {
        return Err(format!(
            "{} has unapproved license {license}",
            path.display()
        ));
    }
    let source = string(value, "source")?;
    // The original Puzzle palettes have no external repository; every
    // other palette needs a direct GitHub blob URL at a fixed commit.
    if source != "Puzzle Terminal" && !source.starts_with("https://github.com/") {
        return Err(format!("{} has no pinned official source", path.display()));
    }
    if source != "Puzzle Terminal" && !source.contains("/blob/") {
        return Err(format!("{} has no pinned commit", path.display()));
    }
    let _ = string(value, "credit")?;
    // Credit is included in the generated constant, where APIs and the
    // gallery can expose the exact provenance without opening a file.
    Ok(())
}

/// Translates one reviewed JSON record into compile-time seed fields.
fn render_one(value: &Value, path: &Path) -> Result<String, String> {
    checked_metadata(value, path)?;
    let palette = value
        .get("palette")
        .and_then(Value::as_array)
        .filter(|items| items.len() == 16)
        .ok_or_else(|| format!("{} needs 16 palette colors", path.display()))?;
    // A terminal stores ANSI index tags in old cells. Missing slots cannot
    // be filled from xterm defaults without changing an authored palette.
    let mut colors = Vec::with_capacity(16);
    for (index, entry) in palette.iter().enumerate() {
        // Wrapping the array item under one key reuses the same strict
        // hex parser used for default and cursor token literals.
        let wrapper = serde_json::json!({"color": entry});
        colors.push(rgb(
            color(&wrapper, "color").map_err(|error| format!("palette {index}: {error}"))?
        ));
    }
    let selection_fg = match value.get("selectionForeground") {
        // Upstream themes vary: some define both selection colors and
        // others define only the fill. `None` asks the runtime to choose
        // readable ink using WCAG contrast after OKLab fill derivation.
        None | Some(Value::Null) => "None".to_owned(),
        Some(entry) => {
            let wrapper = serde_json::json!({"color": entry});
            format!("Some({})", rgb(color(&wrapper, "color")?))
        }
    };
    let field = |key| color(value, key).map(rgb);
    // Foreground and background are mandatory. Optional cursor and
    // selection colors retain `None` when the source has no such token.
    let optional = |key| match value.get(key) {
        None | Some(Value::Null) => Ok("None".to_owned()),
        Some(_) => color(value, key).map(|literal| format!("Some({})", rgb(literal))),
    };
    // Source is a comment in generated code, making a reviewer able to
    // trace every literal back to one pinned upstream file.
    // A theme name in that comment prevents identical generated prose
    // from hiding which of a palette pair a literal belongs to.
    Ok(format!(
        "    // {}: exact terminal colors from {}; absent tokens derive via OKLab.\n    ThemeSeed {{ name: {:?}, display_name: {:?}, appearance: {:?}, credit: {:?}, foreground: {}, background: {}, cursor: {}, cursor_text: {}, selection_background: {}, selection_foreground: {}, palette: [{}] }},\n",
        string(value, "name")?,
        string(value, "source")?,
        string(value, "name")?,
        string(value, "displayName")?,
        string(value, "appearance")?,
        string(value, "credit")?,
        field("foreground")?,
        field("background")?,
        optional("cursor")?,
        optional("cursorText")?,
        optional("selectionBackground")?,
        selection_fg,
        colors.join(", "),
    ))
}

/// Finds exactly the approved number of JSON palettes in stable order.
fn source_paths() -> Result<Vec<PathBuf>, String> {
    // Filesystem enumeration order varies by host; sorting makes source
    // order stable in `Theme::builtin_names()` and in generated docs.
    let mut paths = fs::read_dir(SOURCES)
        .map_err(|error| format!("{SOURCES}: {error}"))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|path| {
        // License copies live below the same asset directory but are not
        // palette documents and must not enter the Rust constant table.
        path.extension()
            .is_some_and(|extension| extension == "json")
    });
    paths.sort();
    if paths.len() != 14 {
        return Err(format!(
            "expected four Puzzle and ten D-25 themes, got {}",
            paths.len()
        ));
    }
    Ok(paths)
}

/// Builds the root index and two seven-theme shards in sorted order.
fn rendered() -> Result<Vec<String>, String> {
    let paths = source_paths()?;
    let mut root = String::from(
        "//! Built-in terminal colors generated by `cargo xtask themes`.\n//!\n//! Shards retain pinned source comments beside exact palette bytes.\nuse super::ThemeSeed;\nmod a;\nmod b;\n\n/// Four Puzzle themes and ten approved D-25 themes.\npub(super) const BUILTINS: &[ThemeSeed] = &[\n",
    );
    let mut first = String::from(
        "//! First seven built-in palettes generated from reviewed JSON.\nuse super::super::ThemeSeed;\n\n/// Pinned terminal colors; missing UI tokens derive through OKLab.\n#[rustfmt::skip]\npub(super) const THEMES: &[ThemeSeed] = &[\n",
    );
    let mut second = String::from(
        "//! Remaining seven built-in palettes generated from reviewed JSON.\nuse super::super::ThemeSeed;\n\n/// Pinned terminal colors; missing UI tokens derive through OKLab.\n#[rustfmt::skip]\npub(super) const THEMES: &[ThemeSeed] = &[\n",
    );
    for (index, path) in paths.into_iter().enumerate() {
        // Each JSON file is parsed independently, so an error names the
        // exact upstream-backed asset a reviewer needs to repair.
        let bytes = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let (part, slot) = if index < 7 {
            (&mut first, index)
        } else {
            (&mut second, index - 7)
        };
        part.push_str(&render_one(&value, &path)?);
        // The index comment makes a root-table order change visible in a
        // small diff even when shard row contents did not change.
        root.push_str(&format!(
            "    // Slot {index} keeps {} in the source-file order.\n",
            string(&value, "name")?
        ));
        root.push_str(&format!(
            "    {}::THEMES[{slot}],\n",
            if index < 7 { "a" } else { "b" }
        ));
    }
    root.push_str("];\n");
    // A fixed root plus two shards avoids a generated file crossing R7
    // while still exposing one contiguous static catalogue at runtime.
    first.push_str("];\n");
    second.push_str("];\n");
    Ok(vec![root, first, second])
}

/// Runs rustfmt on the generated file so source comparison is byte stable.
fn formatted(source: &str, index: usize) -> Result<String, String> {
    // Formatting occurs before both write and compare. Otherwise a normal
    // `cargo fmt` would alter generated files and make check fail again.
    let name = format!("target/themes-generated-{index}.rs");
    let temp = Path::new(&name);
    fs::write(temp, source).map_err(|error| error.to_string())?;
    if index == 0 {
        // rustfmt resolves child modules relative to its temporary root.
        // These empty placeholders exist only in ignored target/; the
        // actual generated shards are formatted in their own passes.
        fs::write("target/a.rs", "").map_err(|error| error.to_string())?;
        fs::write("target/b.rs", "").map_err(|error| error.to_string())?;
    }
    let status = Command::new("rustfmt")
        // Match the workspace edition; Rustfmt's default parser edition
        // can otherwise rewrite syntax differently from `cargo fmt`.
        .arg("--edition")
        .arg("2024")
        .arg(temp)
        .status()
        .map_err(|error| format!("rustfmt generated themes: {error}"))?;
    if !status.success() {
        // A malformed emitted Rust file is a generation failure, not a
        // reason to bless its raw bytes and defer the error to compilation.
        return Err(format!("rustfmt generated themes: {status}"));
    }
    fs::read_to_string(temp).map_err(|error| error.to_string())
}

/// Regenerates constants only after every palette passes source validation.
pub(crate) fn run() -> Result<(), String> {
    // All records are rendered first. A bad fourteenth palette therefore
    // cannot leave a half-updated generated catalogue in version control.
    for (index, source) in rendered()?.iter().enumerate() {
        let source = formatted(source, index)?;
        let path = GENERATED[index];
        // The shard directory may not exist on a fresh checkout; it is a
        // generated source location, not an extra downloaded asset.
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(path, source).map_err(|error| format!("{path}: {error}"))?;
    }
    Ok(())
}

/// Fails the normal check when a palette and its constants have diverged.
pub(crate) fn verify() -> Result<(), String> {
    // Compare every shard. Checking only the root would miss a hand edit to
    // one ANSI slot while the root indices remained unchanged.
    for (index, source) in rendered()?.iter().enumerate() {
        let expected = formatted(source, index)?;
        let actual = fs::read_to_string(GENERATED[index]).map_err(|error| error.to_string())?;
        if actual != expected {
            // The fix is regeneration from reviewed JSON, never weakening
            // the comparison or accepting a manual generated-code patch.
            return Err("generated theme constants are stale; run cargo xtask themes".into());
        }
    }
    Ok(())
}
