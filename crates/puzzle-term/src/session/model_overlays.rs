//! Session owns M8 models and the desktop query mode (design §11.4, D-34).
//!
//! Providers receive capacity-reusing snapshots, not shared mutable models.
//! Session-owned model callbacks return typed actions through the bus.
//! Source stamps and query dirtiness survive sealed view-state publications.
//!
//! The viewport's track/thumbnail are device-space M8 data. Providers require
//! logical pixels, so capture converts each extent once using the builder's DPR.
//! Marker fractions already describe the full retained document; converting them
//! to visible-row coordinates would move offscreen prompts and search results.
//! Source kinds resolve to semantic UI token names, with nonzero exit statuses
//! selecting zoneError and unknown/success statuses selecting zoneOk.
//!
//! Query presentation is a copy of the Session-owned editing mode, plus count
//! and current index from the existing incremental search engine. Both Strings
//! retain capacity across copies. Markers use clear/push into the captured vector,
//! avoiding fresh allocations when already-warmed source lists are sampled.
//! There is no mutable model shared with a callback or the frame-building thread.
//!
//! A movement stamp changes only when the actual viewport scroll position changes.
//! It lets presentation restart visibility after scroll input without treating
//! cursor blink, a theme switch or marker refresh as fresh user scrolling.
//! Query dirty state becomes a single coarse event before view-state publication.
//! Parser changes during render-side builds remain pending with the Session owner.
//!
//! Model providers are installed explicitly by visual hosts. A bare Session stays
//! usable as a parser or grid renderer without engine chrome. Repeated host setup
//! is idempotent, preventing overlapping captures on configuration reload.
//! Sleeping callback deadlines are examined in both frame lanes before their
//! idle return. A due wake requests one build, rather than polling a clean list.
use super::Session;
use puzzle_overlay::{
    models::{Marker, Models},
    provider::EventKinds,
};
use web_time::Instant;
/// Unicode query editing is separate from renderer/provider presentation.
pub(super) mod query;
/// Session mode and the last observed movement, never a second viewport model.
#[derive(Default)]
pub(super) struct State {
    /// Host UI exclusions share one immutable allocation with published views.
    pub occluded_rects: Option<std::sync::Arc<[puzzle_overlay::geometry::Rect]>>,
    /// Observer ownership stays beside the source state it updates.
    pub hud: hud::Hud,
    /// Session clock records, bounded by VT's retained OSC identities.
    pub blocks: blocks::Blocks,
    /// Current link source; the render half never resolves live targets.
    pub link: puzzle_overlay::models::link::Link,
    /// Presented row identity avoids repeated detection on unchanged frames.
    link_probe: links::Probe,
    /// Gesture and scheme restrictions are checked after command interceptors.
    pub link_policy: links::policy::Policy,
    /// One native input mode; general overlay keyboard focus remains deferred.
    pub query: query::Query,
    /// Prepared labels are rebuilt only on table/filter changes.
    help: puzzle_overlay::models::help::Help,
    /// Effective table changes invalidate the next open filter capture.
    help_dirty: bool,
    /// Replacing bindings can change continuations without changing pending keys.
    leader_dirty: bool,
    /// Borrowed matcher identity copied only when its short leader changes.
    pending_seen: Vec<crate::vt::input::KeyEvent>,
    /// Suggestion and status data never enters VT cells or the application transport.
    pub visuals: puzzle_overlay::models::visuals::Visuals,
    /// Actual M8 position last sampled at a source boundary.
    position: Option<f64>,
    /// Monotonic movement stamp for finite auto-hide presentation.
    activity: u64,
    /// Idempotent host installation avoids duplicate regions on settings reload.
    installed: bool,
}
impl Session {
    /// Installs model painters once for a visual host, through the public provider API.
    /// Bare parser/render probes can omit UI; native and browser hosts always install it.
    pub fn install_model_overlays(&mut self) {
        if self.model_overlays.installed {
            return;
        }
        self.model_overlays.installed = true;
        self.overlays_mut().add(
            Box::<puzzle_overlay::builtins::hud::Hud>::default(),
            Box::new(puzzle_overlay::builtins::hud::HudView),
        );
        self.install_status_overlays();
        self.install_help_overlays();
        self.install_link_overlay();
        self.install_block_overlay();
        self.overlays_mut().add(
            Box::new(puzzle_overlay::builtins::scrollbar::Scrollbar::default()),
            Box::new(puzzle_overlay::builtins::scrollbar::ScrollbarView),
        );
        self.overlays_mut().add(
            Box::new(puzzle_overlay::builtins::pill::Pill::default()),
            Box::new(puzzle_overlay::builtins::pill::PillView),
        );
        self.overlays_mut().add(
            Box::new(puzzle_overlay::builtins::search::Search::default()),
            Box::new(puzzle_overlay::builtins::search::SearchView),
        );
    }
    /// Converts exact device geometry once, preserving source fractions and capacities.
    pub(super) fn copy_models(&mut self, out: &mut Models, dpr: f32) {
        out.occluded_rects
            .clone_from(&self.model_overlays.occluded_rects);
        self.copy_blocks(out);
        out.link.clone_from(&self.model_overlays.link);
        self.copy_help(out);
        let position = self.viewport.scroll_px();
        if self.model_overlays.position != Some(position) {
            self.model_overlays.position = Some(position);
            self.model_overlays.activity = self.model_overlays.activity.wrapping_add(1);
            self.overlay_changed(EventKinds::SCROLL);
        }
        if std::mem::take(&mut self.model_overlays.query.dirty) {
            self.overlay_changed(EventKinds::QUERY);
        }
        let scale = f64::from(dpr.max(f32::EPSILON));
        let [x, y, width, height] = self.viewport.scrollbar.track();
        let [top, extent] = self.viewport.scrollbar.thumb();
        out.track = puzzle_overlay::geometry::Rect {
            origin: [(x / scale) as f32, (y / scale) as f32],
            size: [(width / scale) as f32, (height / scale) as f32],
        };
        out.thumb = puzzle_overlay::geometry::Rect {
            origin: [out.track.origin[0], (top / scale) as f32],
            size: [out.track.size[0], (extent / scale) as f32],
        };
        out.markers.clear();
        for marker in self.viewport.scrollbar.markers() {
            use crate::viewport::scrollbar::MarkerKind;
            let token = match marker.kind {
                MarkerKind::Prompt(Some(code)) if code != 0 => "zoneError",
                MarkerKind::Prompt(_) => "zoneOk",
                MarkerKind::Match => "searchMatch",
                MarkerKind::Selection => "selectionBackground",
                MarkerKind::Bell => "bellFlash",
            };
            out.markers.push(Marker {
                start: marker.start,
                end: marker.end,
                token,
            });
        }
        out.scroll_activity = self.model_overlays.activity;
        out.new_output = self.viewport.new_output();
        self.copy_visuals(out);
        let query = &self.model_overlays.query;
        out.search.open = query.open && query.kind == query::Kind::Search;
        out.search.query.clone_from(&query.text);
        out.search.preedit.clone_from(&query.preedit);
        out.search.cursor = query.cursor;
        let state = self.search.state();
        out.search.count = state.count;
        out.search.current = state.current;
        out.search.complete = state.complete;
    }
    /// A sleeping provider's due deadline enters both direct and external frame lanes.
    pub(super) fn advance_model_overlays(&mut self, now: Instant) {
        if let Some(epoch) = self.overlay_epoch {
            let time = now.saturating_duration_since(epoch).as_secs_f64();
            if self
                .overlays
                .next_wake()
                .is_some_and(|deadline| deadline <= time)
            {
                self.needs_frame = true;
            }
        }
    }
    /// Read-only diagnostics and harness input use the same native mode state.
    pub fn search_bar_query(&self) -> Option<&str> {
        let query = &self.model_overlays.query;
        (query.open && query.kind == query::Kind::Search).then_some(query.text.as_str())
    }
}

/// Shortcut table capture and exact leader presentation.
mod help;
/// Cursor-only presentation and application status are sampled separately.
mod visuals;

/// Presented-target validation and on-demand URL detection.
pub(in crate::session) mod links;

/// Session-provided command clocks and visible output intervals.
pub(in crate::session) mod blocks;

/// Completed-frame bus observation feeds frozen HUD source values.
mod hud;
