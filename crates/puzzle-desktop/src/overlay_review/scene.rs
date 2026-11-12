//! Public free-text provider for the installed-font M9 review scene.
//!
//! The first line marks every PNG as machine-specific before any fallback sample.
//! Plain labels use bundled Latin and are independent of installed font revisions.
//! CJK and emoji then exercise native sources at two affine sizes.
//! The requested annotation metrics establish fitting independently of grid height.
//! Rotation and magnification follow the neutral layout, preserving natural advances.
//! Pixel-only resolution uses the strongest magnification of the composed linear map.
//! Threefold zoom therefore needs a fresh larger crab crop, rather than an enlarged small one.
//! CJK remains outline-backed with no bitmap intermediate at either requested size.
//! Color samples retain authored RGB while ordinary CJK inherits the foreground.
//! Top exposes ink edges without truncating them against the terminal grid.
//! This static provider requests no animation and performs no installed-font I/O.
use super::*;
/// Free-text drawing exercises the actual provider layouter and bitmap exception.
pub(super) struct Scene;
/// A machine-specific specimen is still built exclusively on the public overlay API.
impl OverlayView for Scene {
    /// Labels distinguish system-font evidence from bundled reference images.
    /// Origins and sizes are logical; the host performs device conversion once.
    /// Separate regions allow independent submitted-ink checks for both sources.
    /// Whole UTF-8 clusters preserve native shaping and emoji presentation.
    fn build(&mut self, _state: &dyn ViewState, _ctx: &ViewCtx<'_>, out: &mut DrawList) {
        for (text, size, x, y, angle, zoom) in [
            (
                "Machine-specific review: Heiti / Apple Color Emoji",
                14.0,
                20.0,
                15.0,
                0.0,
                1.0,
            ),
            ("Rotated", 14.0, 30.0, 55.0, 0.0, 1.0),
            ("Zoomed 3x", 14.0, 30.0, 215.0, 0.0, 1.0),
            ("漢字", 24.0, 45.0, 95.0, -0.25, 1.0),
            ("漢字", 24.0, 45.0, 255.0, -0.12, 3.0),
            ("🦀", 24.0, 345.0, 95.0, 0.40, 1.0),
            ("🦀", 24.0, 345.0, 255.0, 0.30, 3.0),
        ] {
            // Each requested map is logical; Session supplies DPR and the shared font context.
            out.push(
                Anchor::Viewport(Rect::default()),
                puzzle_term::frame::ZSlot::Top,
                Primitive::Text(TextSpec {
                    text: text.into(),
                    size,
                    face: None,
                    style: 0,
                    color: 0xffaaddbb,
                    transform: Affine::translate((x, y))
                        * Affine::rotate(angle)
                        * Affine::scale(zoom),
                }),
            );
        }
    }
}
