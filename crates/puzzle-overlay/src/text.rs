//! Public free-text measurement seam, independent of font and GPU owners (§11.2).
//!
//! Providers can size panels from the same layout later used for their text draw.
//! View measurement borrows render-owned font caches and sealed snapshot geometry.
use crate::{draw::TextSpec, geometry::Rect};

/// Logical measurements before the TextSpec's final affine transform.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextMetrics {
    /// Natural advance, including trailing spaces.
    pub width: f32,
    /// Line allocation height used to fit foreign-face ink.
    pub height: f32,
    /// Baseline relative to the line's top edge.
    pub baseline: f32,
    /// Union ink bounds; empty text has an empty rectangle.
    pub ink: Rect,
}
/// Rare provider measurements cross the same seam as retained text commands.
pub trait TextLayouter {
    /// Resolves a host-loaded private UI face and scalar by stable icon name.
    /// Missing mappings suppress the icon, rather than falling back to grid text.
    fn icon(&self, _name: &str) -> Option<(u32, char)> {
        None
    }
    /// Measures a bounded single line through loaded faces and whole-cluster fallback.
    fn measure(&self, spec: &TextSpec) -> Result<TextMetrics, String>;
}
