//! Hostile font bytes must produce a report, never a panic (project rule R8, §16).
//!
//! Browser fonts may come from an arbitrary URL. The narrow M2 probe is the
//! first parser on that path, so malformed table offsets and short files
//! must remain ordinary errors rather than taking down the terminal.
use super::ProbeFont;
use proptest::prelude::*;

proptest! {
    /// Random byte strings exercise both short headers and invalid table data.
    #[test]
    fn malformed_font_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        if let Ok(font) = ProbeFont::new(&bytes) {
            // A plausible header can still hide corrupt character-map or
            // outline tables, which are loaded lazily by skrifa.
            let _ = font.glyph('B');
        }
    }
}
