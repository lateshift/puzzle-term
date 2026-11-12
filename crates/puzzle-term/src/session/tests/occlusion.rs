//! Host-native coverage moves floating chrome and search, never the source grid.
use super::*;
use crate::{
    SessionMessage,
    overlay::{
        draw::Primitive,
        geometry::{Cell, Rect},
    },
    search::SearchCommand,
    viewport::ScrollTarget,
};

/// Hits in old/new output sit on the covered right side of an otherwise ordinary grid.
fn scene(dpr: f32) -> (TestHost, TextContext, Instant) {
    let (mut session, _, _) = fixture();
    session.terminal.resize(80, 24);
    let mut context = super::overlays::text_context(dpr);
    session.set_text_context(&context);
    session.install_model_overlays();
    session.set_performance_hud(true).expect("HUD");
    let now = Instant::now();
    for row in 0..100 {
        let text = if row == 20 || row == 70 {
            format!("{:>70}\r\n", "needle")
        } else {
            "row\r\n".into()
        };
        session.feed(text.as_bytes(), now);
    }
    session.draw(now, &mut context).expect("baseline");
    (session, context, now)
}

/// Follow the same built-in query and public navigation paths used by either host.
fn search_next(session: &mut TestHost, now: Instant) {
    session.submit(Command::Search(SearchCommand::Request), Origin::Api);
    session.submit(
        Command::Search(SearchCommand::Find {
            query: "needle".into(),
            options: Default::default(),
        }),
        Origin::Api,
    );
    session.pump(now);
    while !session.search_state().complete {
        session.publish(now);
    }
    session.submit(Command::ScrollTo(ScrollTarget::Match(1)), Origin::Api);
    session.pump(now);
}

/// Test actual retained view primitives, not the avoidance helper's own arithmetic.
fn assert_panels(builder: &FrameBuilder, covered: Rect) {
    let mut panels = 0;
    for (_, list) in builder.overlays.registry.lists() {
        for draw in &list.draws {
            if let Primitive::Rect(spec) = &draw.primitive
                && spec.radius >= 4.0
                && spec.stroke == 0.0
            {
                panels += 1;
                assert!(
                    !spec.rect.overlaps(covered),
                    "floating panel overlaps host UI"
                );
            }
        }
    }
    assert!(panels >= 2, "search and HUD remain visible");
}

#[test]
fn host_top_right_ui_moves_overlays_and_centres_matches_outside_it() {
    for dpr in [1.0, 2.0] {
        let (mut session, mut context, now) = scene(dpr);
        let geometry = session.builder.as_ref().expect("builder").text_geometry();
        let grid = geometry.grid;
        let size = session.terminal().size();
        let [w, h] = geometry.viewport.size;
        let covered = Rect {
            origin: [w / 2.0, 0.0],
            size: [w / 2.0, h * 0.6],
        };
        // A narrow left toolbar blocks full rows but must not obstruct right-side hits.
        let left = Rect {
            origin: [grid.origin[0], h * 0.6],
            size: [8.0, h * 0.4],
        };
        session
            .apply(SessionMessage::OccludedRects(vec![covered, left]), now)
            .expect("coverage");
        search_next(&mut session, now);
        session.draw(now, &mut context).expect("occluded frame");
        let builder = session.builder.as_ref().expect("builder");
        assert_eq!(builder.text_geometry().grid, grid);
        assert_eq!(session.terminal().size(), size);
        assert_panels(builder, covered);
        let hit = session
            .search_fills()
            .iter()
            .find(|hit| hit.current)
            .expect("current");
        let rect = builder
            .text_geometry()
            .cell_rect(Cell {
                line: hit.range.start.line,
                column: hit.range.start.column,
            })
            .expect("visible hit");
        assert!(rect.origin[1] >= covered.origin[1] + covered.size[1]);
        session
            .apply(SessionMessage::OccludedRects(Vec::new()), now)
            .expect("clear");
        session.draw(now, &mut context).expect("restored frame");
        let builder = session.builder.as_ref().expect("builder");
        assert!(builder.overlays.registry.lists().any(|(_, list)| list.draws.iter().any(|draw| {
            matches!(&draw.primitive, Primitive::Rect(spec) if spec.radius == 6.0 && spec.rect.origin[1] == 12.0)
        })), "clearing restores the default search-bar corner");
    }
}
