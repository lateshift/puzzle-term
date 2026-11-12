//! Parsed shell/link specimens use production source ownership and presented hover.
//! Deterministic overlay examples through the production model/view API.
//!
//! The native harness and optional browser review build share these fixtures.
//! They register ordinary public callbacks, without writing render instances
//! directly. This makes screenshots evidence for geometry, anchors and slots.
//! The core panel deliberately occupies padding in Top and a grid cell in
//! BelowText, demonstrating their different default scissors.
//!
//! The fixed panel color is independently asserted by harness readback at
//! DPR 1 and 2. The panel begins eight logical pixels from the surface edge;
//! the grid begins at the harness's sixteen-pixel inset, making the scissor
//! difference directly visible without a font-specific glyph assertion.
//! The first cell rectangle is queried from TextGeometry rather than rebuilt
//! from metrics, proving the same placement API is sufficient for extensions.
//! Re-registering a scene is intentionally a cold operation; the callback's
//! small DrawList is retained and reused on subsequent dirty frames.
//! Each fixture registers through Session rather than fabricating frame batches.
//! Free-text fixtures accept an explicit approved COLR face from their host.
//! This prevents private-use collisions with the simultaneously loaded Nerd Font.
//! Grid effect examples register independently from annotations so their cell text stays visible.
//!
//! Draw-only scenes still register a model half. StaticModel owns their source
//! interests and supplies one immutable unit value, avoiding a parallel provider
//! interface just for examples. Every painter implements the same OverlayView
//! used by built-ins; no view obtains scheduling requests or mutable Session data.
//! Model notification increments its publication version when a source changes,
//! so retained commands rebuild after geometry, theme or font replacement.
//! Registration transfers the drawing half once; skipped publications must still
//! deliver that transfer before the renderer reclaims their snapshot slots.
use crate::{
    Session,
    overlay::{
        anchor::Anchor,
        draw::{DrawList, Primitive, RectSpec},
        geometry::{Cell, Rect},
        model::ViewState,
        provider::EventKinds,
        view::{OverlayView, ViewCtx},
    },
};

/// Registers one named reference provider; unknown names are harness errors.
pub fn install(session: &mut Session, name: &str) -> Result<(), String> {
    if let Some(result) = install_builtin(session, name) {
        return result;
    }
    if name == "models" {
        return models::install(session);
    }
    if name == "decorations" {
        return decorations::install(session);
    }
    if name == "effects" {
        session.overlays_mut().add_view(
            "overlay-effects-scene",
            EventKinds::ALL,
            Box::new(effects::Scene),
        );
        return Ok(());
    }
    if name == "text" {
        session.overlays_mut().add_view(
            "overlay-text-scene",
            EventKinds::ALL,
            Box::new(text::Scene { color_face: None }),
        );
        return Ok(());
    }
    if name == "shapes" {
        session.overlays_mut().add_view(
            "overlay-shapes-scene",
            EventKinds::ALL,
            Box::new(shapes::Scene::new()?),
        );
        return Ok(());
    }
    if name != "core" {
        return Err(format!("unknown overlay scene {name}"));
    }
    session
        .overlays_mut()
        .add_view("overlay-core-scene", EventKinds::ALL, Box::new(Core));
    Ok(())
}

/// Built-in examples configure ordinary sources, separately from custom painters.
/// None leaves the name for the custom geometry fixtures below.
/// HUD registration can fail, so that error stays visible to the harness.
/// All other branches feed protocol or public API data into the same Session.
fn install_builtin(session: &mut Session, name: &str) -> Option<Result<(), String>> {
    if name == "hud-sample" {
        return Some(visuals::hud_sample(session));
    }
    if name == "status" {
        visuals::status(session);
        return Some(Ok(()));
    }
    if name == "hud" {
        // Use the production built-in pair and public visibility toggle.
        session.install_model_overlays();
        // Subscription failure is a scene error, never a fabricated zero-valued sample.
        if let Err(error) = session.set_performance_hud(true) {
            return Some(Err(error));
        }
        // The first actual presentation supplies counters for a later dirty frame.
        // Suppress cursor blink so the specimen has no unrelated timer or ink source.
        // Subsequent scenario feed commands request new frames through ordinary damage.
        session.feed(
            b"\x1b[?25lPerformance HUD: completed-frame counters",
            web_time::Instant::now(),
        );
        return Some(Ok(()));
    }
    if name == "blocks" {
        blocks::install(session);
        return Some(Ok(()));
    }
    if name == "links" {
        links::install(session);
        return Some(Ok(()));
    }
    if matches!(name, "help" | "leader") {
        return Some(help::install(session, name == "leader"));
    }
    if matches!(name, "ghost" | "preedit") {
        return Some(visuals::install(session, name == "preedit"));
    }
    None
}

/// All declaration kinds and placements use the production mutation API.
mod decorations;
/// Seven independent grid effects leave source cells unchanged.
mod effects;
/// Shortcut pills and grouped effective-table specimens.
mod help;
/// Source-fed scrollbar, new-output and native query specimen.
pub mod models;
/// SVG, stroke and dense-plot examples through the public path cache.
mod shapes;
/// Natural Latin, symbol fallback and COLRv1 layers under affine transforms.
mod text;
/// Cursor annotation specimens use the public suggestion and composition sources.
mod visuals;

/// A viewport panel and a cell-sized background built from actual geometry.
struct Core;
/// The stateless painter can be recreated without altering source or interaction state.
impl OverlayView for Core {
    /// Uses fresh logical cells, leaving shaping and GPU compilation to the engine.
    fn build(&mut self, _state: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        let anchor = Anchor::Viewport(Rect::default());
        // A Top panel is allowed to paint in padding, outside the grid scissor.
        out.push(
            anchor,
            puzzle_frame::ZSlot::Top,
            rectangle(
                Rect {
                    origin: [8.0; 2],
                    size: [8.0; 2],
                },
                0xffcc8844,
            ),
        );
        if let Some(line) = ctx.geometry.visible_lines().next()
            && let Some(rect) = ctx.geometry.cell_rect(Cell { line, column: 0 })
        {
            // The text remains a normal grid glyph above this public overlay fill.
            out.push(
                anchor,
                puzzle_frame::ZSlot::BelowText,
                rectangle(rect, 0xff554433),
            );
        }
    }
}

/// Builds a simple logical rectangle without bypassing the draw-list contract.
fn rectangle(rect: Rect, color: u32) -> Primitive {
    Primitive::Rect(RectSpec {
        rect,
        color,
        radius: 0.0,
        stroke: 0.0,
    })
}

/// Registers the bundled COLRv1 text case with its explicit host-loaded face ID.
pub fn install_text(session: &mut Session, color_face: u32) {
    session.overlays_mut().add_view(
        "overlay-text-scene",
        EventKinds::ALL,
        Box::new(text::Scene {
            color_face: Some(color_face),
        }),
    );
}

/// Actual link grouping and presented hover routing for GPU specimens.
pub mod links;

/// Parsed command intervals and policy-preserving hover specimen.
pub mod blocks;
