//! Declared owner turns advance local work independently of external reply waits.
use super::*;
use crate::osc::{OscHostEffect, OscProgress};

/// A synthetic optional protocol waits externally while doing three bounded steps.
struct Working {
    /// Three processing turns are enough to prove that callbacks do not drain a loop.
    steps: usize,
    /// Transfer once after the last local step, independently of writer credit.
    effect: Option<OscHostEffect>,
}
impl OscHandler for Working {
    fn handle(&mut self, _: &OscRequest, _: &mut Vec<u8>, _: &mut Bus<Command, Event>) {
        panic!("requires context");
    }
    fn try_handle_with_context(
        &mut self,
        _: &OscRequest,
        _: &mut OscContext<'_>,
        _: &mut Vec<u8>,
        _: &mut Bus<Command, Event>,
    ) -> OscProgress {
        OscProgress::AwaitReply
    }
    fn control(
        &mut self,
        _: &mut Extension,
        _: &mut OscContext<'_>,
        _: &mut Bus<Command, Event>,
    ) -> Result<(), String> {
        self.steps = 3;
        Ok(())
    }
    fn owner_work_pending(&self) -> bool {
        self.steps != 0
    }
    fn continue_owner_work(&mut self, _: &mut OscContext<'_>, _: &mut Bus<Command, Event>) {
        self.steps -= 1;
        if self.steps == 0 {
            self.effect = Some(OscHostEffect {
                kind: "ext:fixture/local",
                data: Arc::new(vec![1, 2, 3]),
            });
        }
    }
    fn take_host_effect(&mut self) -> Option<OscHostEffect> {
        self.effect.take()
    }
}

#[test]
fn external_wait_does_not_starve_local_work_and_host_wait_is_idle() {
    let (mut s, writes, _) = fixture();
    s.enable_host_events().expect("host observation");
    s.register_osc_handler(
        7779,
        Box::new(Working {
            steps: 0,
            effect: None,
        }),
    )
    .expect("register");
    s.feed(b"\x1b]7779;blocked\x07X", Instant::now());
    assert!(s.waiting_reply_credit());
    s.control_osc_handler(7779, &mut control())
        .expect("start work");
    for index in 0..3 {
        assert!(s.owner_work_pending());
        // Pumping controls cannot refill owner-turn work or run the export.
        s.pump(Instant::now());
        s.feed(&[], Instant::now());
        assert_eq!(s.owner_work_pending(), index != 2);
    }
    assert!(s.waiting_reply_credit());
    assert_eq!(
        s.terminal().cursor.column,
        0,
        "blocked ordered suffix was parsed"
    );
    let effects: Vec<_> = std::iter::from_fn(|| s.take_event())
        .filter_map(|e| {
            if let crate::HostEvent::OscEffect(e) = e {
                Some(e)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(effects.len(), 1);
    assert_eq!(*effects[0].data, vec![1, 2, 3]);
    assert!(!s.owner_work_pending(), "waiting for host must not poll");
    s.submit(Command::Text("key".into()), Origin::User);
    s.pump(Instant::now());
    assert_eq!(writes.lock().expect("writer").concat(), b"key");
}
