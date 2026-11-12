//! Bundled-font-only affine text specimens through the public provider API.
//!
//! Latin checks natural shaping, the icon requires a second loaded face, and
//! the vendored test font supplies COLRv1 gradients instead of machine emoji.
//! System CJK/bitmap emoji use a separately labeled review-only host scenario.
//! Natural advances keep marks and ligatures attached while rotation moves ink.
//! Threefold zoom exercises unhinted outlines independently of terminal metrics.
//! Nerd Font and COLR use separate face identities despite overlapping PUA maps.
//! The color fixture includes both a linear gradient and layered sweep paints.
//! Font bytes belong to the harness host; this provider stores only face IDs.
//! Review at both scales remains a human decision after mechanical ink checks.
use crate::overlay::{
    anchor::Anchor,
    draw::{DrawList, Primitive, TextSpec},
    geometry::Rect,
    model::ViewState,
    view::{OverlayView, ViewCtx},
};
use kurbo::Affine;
use puzzle_frame::ZSlot;
/// One retained text list contains independent rotation and magnification cases.
pub(super) struct Scene {
    /// Explicit fixture face avoids collisions with Nerd Font supplementary PUA icons.
    pub(super) color_face: Option<u32>,
}
/// The host supplies bundled fallbacks; the provider never reads font files.
impl OverlayView for Scene {
    /// Each request enters ordinary shaping and the shared Slug/paint stores.
    fn build(&mut self, _state: &dyn ViewState, _ctx: &ViewCtx<'_>, out: &mut DrawList) {
        let anchor = Anchor::Viewport(Rect::default());
        // Whole logical transforms apply after natural glyph advances and mark offsets.
        for (text, size, x, y, angle, zoom) in [
            ("Free text -> é", 24.0, 40.0, 45.0, -0.10, 1.0),
            ("Zoom 3×", 14.0, 45.0, 115.0, 0.0, 3.0),
            ("Icon \u{f121}", 24.0, 385.0, 50.0, 0.30, 1.5),
            ("\u{f0100}\u{f0200}", 45.0, 400.0, 155.0, -0.20, 1.5),
        ] {
            // Foreground text and authored color layers share one ordered z-slot.
            out.push(
                anchor,
                ZSlot::AboveText,
                Primitive::Text(TextSpec {
                    text: text.into(),
                    size,
                    face: if text.starts_with('\u{f0100}') {
                        self.color_face
                    } else {
                        None
                    },
                    style: 0,
                    color: 0xffeeddcc,
                    transform: Affine::translate((x, y))
                        * Affine::rotate(angle)
                        * Affine::scale(zoom),
                }),
            );
        }
    }
}
