//! OSC timing, hover copy and policy are exercised through sealed Session frames.
use super::*;
use crate::{
    overlay::{
        draw::Primitive,
        provider::{PointerEvent, PointerKind},
    },
    platform::Clipboard,
    selection::{CopyTarget, SelectionCommand},
};
/// Copy receipts stay outside Session without touching the system clipboard.
struct ClipboardProbe(Arc<Mutex<Vec<String>>>);
impl Clipboard for ClipboardProbe {
    fn write_text(&mut self, value: &str) -> Result<(), String> {
        self.0.lock().expect("copy receipts").push(value.into());
        Ok(())
    }
    fn request_text(&mut self) -> Result<Option<String>, String> {
        Ok(None)
    }
}
/// Looks up a real presented button's bounds instead of duplicating its layout math.
fn button(host: &TestHost) -> [f32; 2] {
    host.builder
        .as_ref()
        .expect("builder")
        .overlays
        .registry
        .lists()
        .flat_map(|(_, list)| &list.draws)
        .find_map(|draw| match draw.primitive {
            Primitive::Region(bounds, id, _) if id.0 & (1 << 63) != 0 => {
                Some([bounds.origin[0] + 2.0, bounds.origin[1] + 2.0])
            }
            _ => None,
        })
        .expect("copy button")
}
/// The final duration excludes prompt input; copy selects the hovered, not latest, block.
#[test]
fn command_block_duration_hover_copy_and_policy() {
    for dpr in [1.0, 2.0] {
        let (mut host, writes, _) = fixture();
        let copied = Arc::new(Mutex::new(Vec::new()));
        host.services.clipboard = Box::new(ClipboardProbe(copied.clone()));
        let mut context = super::overlays::text_context(dpr);
        let now = Instant::now();
        host.set_text_context(&context);
        host.install_model_overlays();
        let later = populate_blocks(&mut host, now);
        host.draw(later, &mut context).expect("block frame");
        host.presented();
        let blocks = &host.snapshot.overlays.models.blocks;
        assert_eq!(blocks.len(), 2);
        assert_eq!((blocks[0].exit, blocks[0].duration), (Some(1), Some(1.25)));
        let first = blocks[0];
        let row = host
            .overlay_geometry()
            .line_rect(first.first)
            .expect("visible block");
        let event = PointerEvent {
            kind: PointerKind::Move,
            point: [row.origin[0] + 10.0, row.origin[1] + 2.0],
            button: None,
            modifiers: 0,
        };
        host.route_overlay_pointer(&event, later);
        host.draw(later, &mut context).expect("hover frame");
        host.presented();
        host.submit(Command::Select(SelectionCommand::All), Origin::Api);
        host.pump(later);
        let selection = host.selection_text();
        let point = button(&host);
        let result = host.route_overlay_pointer(
            &PointerEvent {
                kind: PointerKind::Press,
                point,
                button: Some(0),
                modifiers: 0,
            },
            later,
        );
        assert!(result.handled);
        assert_eq!(
            copied.lock().expect("copy receipts").as_slice(),
            ["first\nsecond\n"]
        );
        assert_eq!(host.selection_text(), selection);
        // A direct explicit target takes the same clipboard-write interceptor chain.
        deny_copy(&mut host);
        host.submit(
            Command::Copy(CopyTarget::ZoneOutput(first.id)),
            Origin::User,
        );
        host.pump(later);
        assert_eq!(copied.lock().expect("copy receipts").len(), 1);
        assert!(writes.lock().expect("transport").is_empty());
    }
}
/// An equal C/D boundary is valid empty output, never the next command's text.
#[test]
fn explicit_empty_output_and_evicted_zone_are_inert() {
    let (mut host, _, _) = fixture();
    let now = Instant::now();
    host.feed(b"\x1b]133;C\x07\x1b]133;D;0\x07", now);
    let id = host.buffer().zones().next().expect("C").id;
    let view = host.buffer();
    assert_eq!(
        view.command_output(view.zones().next().expect("C")),
        Some(String::new())
    );
    assert_eq!(
        host.selection
            .copy(&host.terminal, CopyTarget::ZoneOutput(id)),
        Some(String::new())
    );
    host.feed(b"\x1bc", now);
    assert_eq!(
        host.selection
            .copy(&host.terminal, CopyTarget::ZoneOutput(id)),
        None
    );
}

/// Installs a policy that vetoes clipboard writes, independently of the copy source.
fn deny_copy(host: &mut TestHost) {
    use puzzle_bus::hooks::{Flow, HookCtx, Interceptor};
    struct Deny;
    impl Interceptor<Command, Event> for Deny {
        fn intercept(&mut self, _: &mut Command, _: &mut HookCtx<'_, Command, Event>) -> Flow {
            Flow::Stop("copy denied")
        }
    }
    host.bus_mut()
        .intercept(
            puzzle_bus::hooks::CommandKinds::at(18),
            0,
            "deny-copy",
            Deny,
        )
        .expect("policy");
}

/// Feeds distinct commands so hover copy cannot accidentally select the newest output.
fn populate_blocks(host: &mut TestHost, now: Instant) -> Instant {
    host.feed(
        b"\x1b]133;A\x07$ \x1b]133;B\x07echo\r\n\x1b]133;C\x07first\r\nsecond\r\n",
        now,
    );
    let later = now + Duration::from_millis(1250);
    host.feed(b"\x1b]133;D;1\x07\x1b]133;A\x07$ \x1b]133;B\x07other\r\n\x1b]133;C\x07latest\r\n\x1b]133;D;0\x07",later);

    later
}

/// Native UI avoidance preserves command identity and the presented button bounds.
mod occlusion;

/// Post-policy outcomes and finite model-driven feedback.
mod feedback;
