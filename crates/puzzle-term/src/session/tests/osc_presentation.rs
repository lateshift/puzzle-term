//! Registered source models seal state, route presented hits and reject forged UI origin.
use super::*;
use crate::osc::{
    OscContext, OscHandler, OscOverlay, OscRequest, OscUiEffect, OscUserAction, SourceWindow,
};
use puzzle_overlay::{
    anchor::Anchor,
    draw::{CursorIcon, DrawList, Primitive, RegionId},
    geometry::Rect,
    model::{ModelCtx, OverlayModel, SharedState, ViewState},
    provider::{EventKinds as Interests, Handled, PointerEvent, PointerKind, SourcePointer},
    view::{OverlayView, ViewCtx},
};

struct Model {
    state: SharedState,
    copies: Arc<AtomicUsize>,
}
impl OverlayModel for Model {
    fn name(&self) -> &'static str {
        "source-fixture"
    }
    fn wants(&self) -> Interests {
        Interests(Interests::GEOMETRY.0 | Interests::THEME.0)
    }
    fn view_state(&mut self) -> SharedState {
        self.copies.fetch_add(1, Ordering::Relaxed);
        self.state.clone()
    }
    fn accept_state(&mut self, state: SharedState) -> bool {
        self.state = state;
        true
    }
    fn on_pointer(
        &mut self,
        event: &PointerEvent,
        region: RegionId,
        ctx: &mut ModelCtx<'_>,
    ) -> Handled {
        if event.kind == PointerKind::Release {
            ctx.requests.source_pointer.set(Some(SourcePointer {
                channel: 7778,
                region,
                event: *event,
            }));
        }
        Handled::Yes
    }
}
struct View(Arc<Mutex<Vec<u64>>>);
impl OverlayView for View {
    fn build(&mut self, state: &dyn ViewState, _: &ViewCtx<'_>, out: &mut DrawList) {
        let value = *state.as_any().downcast_ref::<u64>().expect("source state");
        self.0.lock().expect("view observations").push(value);
        let bounds = Rect {
            origin: [20.0, 20.0],
            size: [20.0, 20.0],
        };
        out.push(
            Anchor::Viewport(bounds),
            puzzle_frame::ZSlot::Top,
            Primitive::Region(
                Rect {
                    origin: [0.0, 0.0],
                    size: bounds.size,
                },
                RegionId(value),
                CursorIcon::Pointer,
            ),
        );
    }
}
struct Handler {
    value: u64,
    published: Option<u64>,
    copies: Arc<AtomicUsize>,
    observations: Arc<Mutex<Vec<u64>>>,
    actions: Arc<AtomicUsize>,
}
/// The core routes an opaque extension outcome without knowing its protocol name.
struct UserFact;
impl puzzle_bus::wire::WireExt for UserFact {
    const TYPE_NAME: &'static str = "ext:test/userOutcome";
    fn to_value(&self) -> puzzle_bus::Value {
        puzzle_bus::Value::Null
    }
    fn from_value(_: &puzzle_bus::Value) -> Result<Self, puzzle_bus::value::ValidationReport> {
        Ok(Self)
    }
}
impl OscHandler for Handler {
    fn handle(&mut self, _: &OscRequest, _: &mut Vec<u8>, _: &mut Bus<Command, Event>) {
        self.value += 1;
    }
    fn overlay(&mut self) -> Option<OscOverlay> {
        Some((
            Box::new(Model {
                state: Arc::new(0_u64),
                copies: self.copies.clone(),
            }),
            Box::new(View(self.observations.clone())),
        ))
    }
    fn overlay_state(&mut self, _: &mut OscContext<'_>, _: SourceWindow) -> Option<SharedState> {
        if self.published == Some(self.value) {
            return None;
        }
        self.published = Some(self.value);
        Some(Arc::new(self.value))
    }
    fn user_action(
        &mut self,
        action: OscUserAction<'_>,
        _: &mut OscContext<'_>,
    ) -> Option<OscUiEffect> {
        let valid = match action {
            OscUserAction::Named("source-next") => true,
            OscUserAction::Pointer { region, .. } => region.0 == self.value,
            _ => return None,
        };
        if valid {
            self.actions.fetch_add(1, Ordering::Relaxed);
        }
        let event = puzzle_bus::wire::Extension::new(UserFact).expect("fact");
        Some(OscUiEffect {
            event: valid.then_some(event),
            ..Default::default()
        })
    }
}

#[test]
fn source_overlay_seals_values_and_does_not_copy_a_clean_model() {
    let (mut host, _, _) = fixture();
    host.enable_host_events().expect("host facts");
    let mut context = overlays::text_context(1.0);
    let now = Instant::now();
    host.set_text_context(&context);
    let copies = Arc::new(AtomicUsize::new(0));
    let observations = Arc::new(Mutex::new(Vec::new()));
    let actions = Arc::new(AtomicUsize::new(0));
    host.register_osc_handler(
        7778,
        Box::new(Handler {
            value: 0,
            published: None,
            copies: copies.clone(),
            observations: observations.clone(),
            actions: actions.clone(),
        }),
    )
    .expect("register");
    host.feed(b"\x1b]7778;update\x1b\\", now);
    host.publish(now).expect("seal first value");
    host.feed(b"\x1b]7778;update\x1b\\", now);
    // An old sealed slot sees value 1 even after the source owner reaches 2.
    let mut builder = FrameBuilder::new(&context);
    let mut frame = RenderFrame::default();
    builder
        .build_published(&mut context, &mut host.snapshot, &mut frame)
        .expect("old frame");
    assert_eq!(*observations.lock().expect("values"), [1]);
    host.builder = Some(builder);
    host.draw(now, &mut context).expect("second value");
    assert_eq!(*observations.lock().expect("values"), [1, 2]);
    let count = copies.load(Ordering::Relaxed);
    host.feed(b"ordinary text", now);
    host.draw(now, &mut context).expect("unrelated grid frame");
    assert_eq!(copies.load(Ordering::Relaxed), count, "clean source copied");
    verify_presented_controls(&mut host, &actions, now);
}

#[test]
fn source_extension_keybindings_keep_real_user_origin() {
    let (mut host, _, _) = fixture();
    let actions = Arc::new(AtomicUsize::new(0));
    host.register_osc_handler(
        7778,
        Box::new(Handler {
            value: 0,
            published: None,
            copies: Arc::new(AtomicUsize::new(0)),
            observations: Arc::new(Mutex::new(Vec::new())),
            actions: actions.clone(),
        }),
    )
    .expect("register");
    for origin in [
        Origin::Api,
        Origin::Terminal,
        Origin::Platform,
        Origin::User,
    ] {
        host.submit(
            Command::BindingAction {
                action: crate::keybind::Action::Ext("source-next".into()),
                all: false,
            },
            origin,
        );
        host.pump(Instant::now());
    }
    assert_eq!(actions.load(Ordering::Relaxed), 1);
    // A configured key reaches the same policy-approved owner path.
    let mut table = crate::keybind::Keybindings::default();
    table.apply(
        &["a=ext:source-next".into()],
        crate::keybind::Source::User,
        crate::keybind::Platform::Mac,
        false,
    );
    host.set_keybindings(table);
    host.submit(Command::Key(KeyEvent::press(Key::Char('a'))), Origin::User);
    host.pump(Instant::now());
    assert_eq!(actions.load(Ordering::Relaxed), 2);
}

#[test]
fn reset_before_an_extension_in_the_same_cascade_cannot_act_on_old_source() {
    let (mut host, _, _) = fixture();
    let actions = Arc::new(AtomicUsize::new(0));
    host.register_osc_handler(
        7778,
        Box::new(Handler {
            value: 0,
            published: None,
            copies: Arc::new(AtomicUsize::new(0)),
            observations: Arc::new(Mutex::new(Vec::new())),
            actions: actions.clone(),
        }),
    )
    .expect("register");
    // Lifecycle callbacks run after the drain. The source fingerprint must
    // reject this queued action before that later callback can retire metadata.
    for action in [
        crate::keybind::Action::Reset,
        crate::keybind::Action::Ext("source-next".into()),
    ] {
        host.submit(Command::BindingAction { action, all: false }, Origin::User);
    }
    host.pump(Instant::now());
    assert_eq!(actions.load(Ordering::Relaxed), 0);
}

/// Presented regions alone authorize the pointer outcome; stale tokens remain inert.
fn verify_presented_controls(host: &mut TestHost, actions: &Arc<AtomicUsize>, now: Instant) {
    // Building a region is insufficient: hit routing uses the presented table.
    let pointer = PointerEvent {
        kind: PointerKind::Release,
        point: [25.0, 25.0],
        button: Some(0),
        modifiers: 0,
    };
    assert!(!host.route_overlay_pointer(&pointer, now).handled);
    host.presented();
    while host.take_event().is_some() {}
    assert!(host.route_overlay_pointer(&pointer, now).handled);
    host.pump(now);
    assert!(
        std::iter::from_fn(|| host.take_event()).any(|event| matches!(event,
        crate::HostEvent::Fact(Event::Ext(value)) if value.downcast_ref::<UserFact>().is_some()))
    );
    assert_eq!(actions.load(Ordering::Relaxed), 1);
    // Rewriting the source without a presentation leaves a truthful stale token.
    host.feed(b"\x1b]7778;update\x1b\\", now);
    assert!(host.route_overlay_pointer(&pointer, now).handled);
    assert_eq!(
        actions.load(Ordering::Relaxed),
        1,
        "stale presented token acted"
    );
}
