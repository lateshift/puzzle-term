//! Reproducible bundled-face packs for first-frame ASCII (design §8.8).
//!
//! The source files are the already-approved D-6 assets. Each variable
//! TTF is prepared at regular or bold weight with unhinted outlines; no
//! bitmap or ppem-specific data enters the pack. The decoder in
//! puzzle-text validates the exact bytes after generation, so a partial
//! or stale file cannot be reported as a successful build artifact.
//!
//! The four downloads correspond to regular, bold, italic and bold
//! italic style slots. Their source files are shared in pairs, but each
//! weight has distinct unhinted outlines and a distinct source hash.
//! The command measures the sum of separately brotli-compressed packs,
//! matching four independently fetched browser assets.
use puzzle_text::{FontData, pack::FontPack};
use std::{fs, path::Path};

/// Four style slots represented by two source files and two weights.
const FACES: [(&str, &str, u16); 4] = [
    ("JetBrainsMono-Variable.ttf", "jetbrains-regular.pzfp", 400),
    ("JetBrainsMono-Variable.ttf", "jetbrains-bold.pzfp", 700),
    (
        "JetBrainsMono-Italic-Variable.ttf",
        "jetbrains-italic.pzfp",
        400,
    ),
    (
        "JetBrainsMono-Italic-Variable.ttf",
        "jetbrains-bold-italic.pzfp",
        700,
    ),
];

/// Generates and validates all four packs before checking total brotli size.
pub(crate) fn run() -> Result<(), String> {
    // Creating only this approved output directory keeps the generator
    // from touching unrelated assets or the TypeScript project.
    let directory = Path::new("assets/fontpacks");
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut total_raw = 0_usize;
    let mut total_brotli = 0_usize;
    for (source, output, weight) in FACES {
        // A fresh source read makes the pack hash reflect the committed
        // font bytes rather than an old in-memory or build-cache copy.
        let source = Path::new("assets/fonts").join(source);
        let data = fs::read(&source).map_err(|error| format!("{}: {error}", source.display()))?;
        let pack = FontPack::build(FontData::new(data, 0), weight)?;
        // A decode immediately after encode catches an accidental writer
        // format change before the candidate becomes a checked-in asset.
        let bytes = pack.encode();
        let loaded = FontPack::decode(&bytes)?;
        if loaded.glyphs.len() != pack.glyphs.len() {
            return Err(format!("{output}: decoded glyph directory changed"));
        }
        let path = directory.join(output);
        fs::write(&path, &bytes).map_err(|error| format!("{}: {error}", path.display()))?;
        let name = path.to_str().ok_or("font pack path is not UTF-8")?;
        // Compression is measured on the artifact a browser would fetch,
        // not on Rust's in-memory Vec or the source TTF size.
        let brotli = crate::commands::compressed("brotli", &["-c", name])?;
        total_raw += bytes.len();
        total_brotli += brotli;
        println!("font-pack {output}: raw={} brotli={brotli}", bytes.len());
    }
    println!("font-pack total: raw={total_raw} brotli={total_brotli}");
    // The four-face target is a milestone budget. A larger pack needs
    // the user's decision; this command cannot quietly raise its limit.
    if total_brotli > 250_000 {
        return Err(format!(
            "four packs exceed the 250 KB brotli budget by {} bytes",
            total_brotli - 250_000
        ));
    }
    Ok(())
}
