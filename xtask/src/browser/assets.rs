//! Temporarily expose approved demo assets through the browser test runner.
//!
//! wasm-pack runs Cargo in puzzle-web, and wasm-bindgen-test serves that
//! directory. Copying the seven existing assets there exercises normal HTTP
//! fetches, pack decoding and asynchronous font takeover without a fetch mock
//! or another server/tool. These copies never enter packaging or commits.
use std::path::PathBuf;

/// Only files created by this invocation are removed on exit.
pub(super) struct Served(Vec<PathBuf>);

/// Assets remain present until the real browser process has finished.
impl Served {
    /// Refuses to replace an existing file; a failed setup cleans its own prefix.
    pub(super) fn new() -> Result<Self, String> {
        let mut served = Self(Vec::new());
        for source in [
            // Style packs must be served too: a 404 would exercise full-font fallback.
            "assets/fontpacks/jetbrains-regular.pzfp",
            "assets/fontpacks/jetbrains-bold.pzfp",
            "assets/fontpacks/jetbrains-italic.pzfp",
            "assets/fontpacks/jetbrains-bold-italic.pzfp",
            "assets/fonts/JetBrainsMono-Variable.ttf",
            "assets/fonts/JetBrainsMono-Italic-Variable.ttf",
            "assets/fonts/SymbolsNerdFontMono-Regular.ttf",
        ] {
            // The demo and runtime request these exact flat filenames (§12.6).
            let name = std::path::Path::new(source)
                .file_name()
                .ok_or("asset filename absent")?;
            let path = std::path::Path::new("crates/puzzle-web").join(name);
            // create_new prevents replacing a file owned by the user or another run.
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| format!("browser HTTP asset {source}: {e}"))?;
            served.0.push(path);
            // Record ownership before copying, so even a partial I/O failure is cleaned up.
            let mut input = std::fs::File::open(source).map_err(|e| e.to_string())?;
            std::io::copy(&mut input, &mut file).map_err(|e| e.to_string())?;
        }
        Ok(served)
    }
}

/// Failed compilation or assertions must not leave asset copies in source control.
impl Drop for Served {
    /// Paths were exclusively created by new; unrelated files are never removed.
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}
