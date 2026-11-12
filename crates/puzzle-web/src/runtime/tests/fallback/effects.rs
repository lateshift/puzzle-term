//! Public provider zoom must redraw the browser's complete fallback cluster at effective size.
use super::*;
use puzzle_term::overlay::{
    anchor::Anchor,
    draw::{DrawList, Effect, Primitive},
    geometry::{Cell, Range, Rect},
    model::ViewState,
    provider::EventKinds,
    view::{OverlayView, ViewCtx},
};
/// No source-size assertion can pass from enlarging only the destination quad.
#[wasm_bindgen_test(async)]
async fn zoomed_fallback_redraws_at_effective_device_size() {
    let (terminal, host) = super::super::presentation::empty_terminal().await;
    let resource = terminal.owner.borrow().text.clone().unwrap();
    JsFuture::from(super::super::super::resources::load_fonts(&resource))
        .await
        .unwrap();
    terminal.owner.borrow_mut().visible = false;
    JsFuture::from(terminal.write("\x1b[?25l🦀".into()).unwrap())
        .await
        .unwrap();
    super::settle(&terminal).await.unwrap();
    let ordinary = crop(&terminal, &resource);
    terminal.owner.borrow_mut().session.overlays_mut().add_view(
        "browser-sized-effect",
        EventKinds::ALL,
        Box::new(Zoom),
    );
    super::settle(&terminal).await.unwrap();
    let zoomed = crop(&terminal, &resource);
    // UV dimensions name actual source texels; destination rectangles are neutral-sized.
    assert!(
        zoomed[0] >= ordinary[0] * 2.5 && zoomed[1] >= ordinary[1] * 2.5,
        "browser reused a small raster: {ordinary:?} -> {zoomed:?}"
    );
    terminal.owner.borrow_mut().visible = true;
    // Inspection consumed the Session frame; another public write submits its retained uploads.
    JsFuture::from(terminal.write("\x1b[24;1H!".into()).unwrap())
        .await
        .unwrap();
    super::super::presentation::presented_pixels(&terminal).await;
    terminal.dispose().unwrap();
    host.remove();
}
/// Captures actual retained atlas crop dimensions before pending writes are submitted.
fn crop(
    terminal: &PuzzleTerminal,
    resource: &Rc<super::super::super::resources::Text>,
) -> [f32; 2] {
    let mut runtime = terminal.owner.borrow_mut();
    let mut context = resource.context.borrow_mut();
    runtime.session.submit(
        puzzle_term::messages::Command::GlyphRasterized,
        puzzle_term::Origin::Platform,
    );
    runtime.session.pump(Instant::now());
    let frame = runtime
        .build_frame(Instant::now(), context.as_mut().unwrap())
        .unwrap()
        .unwrap();
    let draw = frame
        .rasters
        .iter()
        .find(|draw| draw.flags & 1 != 0)
        .unwrap();
    [
        (draw.uv[2] - draw.uv[0]) * 2048.0,
        (draw.uv[3] - draw.uv[1]) * 2048.0,
    ]
}
/// A threefold scale composed with rotation reaches the common bitmap-only effect seam.
struct Zoom;
impl OverlayView for Zoom {
    fn build(&mut self, _state: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        let line = ctx.geometry.visible_lines().next().unwrap();
        out.push(
            Anchor::Viewport(Rect::default()),
            puzzle_term::frame::ZSlot::Text,
            Primitive::GlyphEffect(
                Range {
                    start: Cell { line, column: 0 },
                    end: Cell { line, column: 1 },
                },
                Effect {
                    transform: puzzle_term::text::geometry::Affine::rotate(0.3)
                        * puzzle_term::text::geometry::Affine::scale(3.0),
                    ..Default::default()
                },
            ),
        );
    }
}
