//! M15.4 counts actual warmed frame assembly separately from idle publication.
use super::*;
use crate::{PublishOutcome, frame_snapshot::FrameSnapshot};

/// Images reuse source/instance storage across full assembly and request no idle frames.
#[test]
fn image_screen_warm_assembly_and_idle_allocate_nothing() {
    for shown in [false, true] {
        let (mut session, _, _) = fixture();
        let mut context = super::overlays::text_context(1.0);
        session.set_text_context(&context);
        session.enable_images(4096);
        let now = Instant::now();
        session.feed(b"\x1b[?25l", now);
        if shown {
            super::images::pixel(&mut session, "i=1,c=2,r=2", now);
        }
        let mut slot = FrameSnapshot::default();
        session
            .publish_frame(now, &mut slot, 0, 0)
            .expect("publish");
        let mut builder = FrameBuilder::new(&context);
        let mut frame = RenderFrame::default();
        for _ in 0..10 {
            builder
                .build_published(&mut context, &mut slot, &mut frame)
                .expect("warm build");
        }
        // Force actual assembly from the unchanged sealed source, rather than
        // accidentally counting only an early idle return as a warm frame.
        let assembly = allocation_counter::measure(|| {
            for _ in 0..100 {
                builder
                    .build_published(&mut context, &mut slot, &mut frame)
                    .expect("build");
            }
        });
        assert_eq!(assembly.count_total, 0, "shown={shown}");
        assert_eq!(
            frame.images.as_ref().map_or(0, |i| i.sources.len()),
            usize::from(shown)
        );
        let idle = allocation_counter::measure(|| {
            for _ in 0..100 {
                assert_eq!(
                    session.publish_frame(now, &mut slot, 0, 0).expect("idle"),
                    PublishOutcome::NothingNew
                );
            }
        });
        assert_eq!(idle.count_total, 0, "shown={shown}");
    }
}
