//! Named UI glyphs use private loaded faces, never the grid fallback chain.
//!
//! Hosts supply the map and font bytes at context construction. Replacing the
//! map changes UI vocabulary without touching a provider or its glyph pipeline.
//! Validation precedes installation: an incomplete font cannot produce tofu for
//! a built-in control. Ordinary scalar selection never visits this small map.
use super::*;

/// Explicit UI loading leaves primary metrics and terminal selection caches intact.
impl FontSet {
    /// Loads a private UI face and maps names to its checked Unicode scalars.
    /// Returns an error before changing the face list if a mapped glyph is absent.
    pub fn install_ui_icons(
        &mut self,
        data: FontData,
        icons: &[(&str, char)],
    ) -> Result<(), String> {
        // Validate every mapped code point before making its face identity visible.
        let face = FontFace::new(data, 400)?;
        for (name, scalar) in icons {
            // A UI map is host data, not a request to search terminal fallback.
            if name.is_empty() || face.glyph_id(*scalar)?.is_none() {
                return Err("UI icon name or glyph is missing".into());
            }
        }
        let id = u32::try_from(self.faces.len()).map_err(|_| "too many UI faces")?;
        self.faces.push(face);
        // Deliberately do not insert id into fallback or invalidate scalar hits.
        for (name, scalar) in icons {
            if let Some(entry) = self.ui_icons.iter_mut().find(|entry| entry.0 == *name) {
                // An existing icon name replaces its mapping, never grid coverage.
                entry.1 = id;
                entry.2 = *scalar;
            } else {
                self.ui_icons.push(((*name).into(), id, *scalar));
            }
        }
        Ok(())
    }

    /// Resolves one UI name; callers explicitly select this face for free text.
    pub fn ui_icon(&self, name: &str) -> Option<(u32, char)> {
        // Providers ask for a UI name only while rebuilding their retained draw list.
        self.ui_icons
            .iter()
            .find(|entry| entry.0 == name)
            .map(|entry| (entry.1, entry.2))
    }
}
