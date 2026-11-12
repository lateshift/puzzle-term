//! Budget reload retires geometry while retaining reproducible sources.
//! A context generation change invalidates every session's old row handles.
use super::{SlugAtlas, SlugBackend};
/// The backend owns address retirement; fonts and packs remain source owners.
impl SlugBackend {
    /// Accounts for a separate scratch page without discarding fitting live geometry.
    pub fn restrict_budget(&mut self, bytes: usize) -> Result<(), String> {
        // A source-policy switch may leave live planes above the smaller cap.
        if self.atlas.set_capacity_limit(bytes).is_err() {
            self.replace_budget(bytes)?;
        }
        Ok(())
    }

    /// Replaces sampled geometry between builds; fonts and pack sources survive.
    pub fn replace_budget(&mut self, bytes: usize) -> Result<(), String> {
        // Validate the replacement before disturbing the previously drawable owner.
        let atlas = SlugAtlas::with_budget(bytes).map_err(|error| error.to_string())?;
        // Pack-only layout reads ready handles directly. Re-import its pinned
        // geometry before publishing the new owner, without waiting for fonts.
        let mut replacement = Self {
            atlas,
            ..Self::default()
        };
        if self.atlas.has_pack_pins() {
            for (&face, source) in &self.packs {
                replacement.add_pack(face, source.pack.as_ref().clone())?;
            }
        } else {
            // Loaded fonts recover packed shapes lazily instead of eagerly refilling.
            replacement.packs = std::mem::take(&mut self.packs);
        }
        // Renderer selection is policy; it does not belong to discarded addresses.
        replacement.grid = std::mem::take(&mut self.grid);
        *self = replacement;
        Ok(())
    }
}
