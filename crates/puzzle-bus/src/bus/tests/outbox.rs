//! Buffered fact delivery preserves observer filters without foreign callbacks.
use super::{Bus, Command, Event};
use crate::hooks::{EventKinds, HookCtx, Observer};
use std::collections::VecDeque;

/// Test host keeps owned bell facts until the drain's mutable borrow ends.
#[derive(Default)]
struct Outbox(VecDeque<Event>);

/// Browser-facing hooks must still obey the same synchronous observer contract.
impl Observer<Command, Event> for Outbox {
    /// Copy only a unit fact; no domain or foreign service downcast is needed.
    fn observe(&mut self, event: &Event, _: &mut HookCtx<'_, Command, Event>) {
        if matches!(event, Event::Bell) {
            self.0.push_back(Event::Bell);
        }
    }

    /// Ownership moves to the host after normal observer delivery has finished.
    fn take_event(&mut self) -> Option<Event> {
        self.0.pop_front()
    }
}

/// Unmatched facts never queue, and removing a hook destroys unread work.
#[test]
fn buffered_observers_keep_filters_and_disposal() {
    let mut bus = Bus::<Command, Event>::default();
    let bells = bus
        .observe(EventKinds::at(0), 0, "bells", Outbox::default())
        .expect("bell hook");
    let other = bus
        .observe(EventKinds::at(1), 0, "other", Outbox::default())
        .expect("other hook");
    bus.publish(Event::Bell);
    bus.publish(Event::Bell);
    assert!(bus.take_observed(bells).is_none());
    bus.drain(|_, _| {});
    assert!(bus.take_observed(other).is_none());
    assert!(matches!(bus.take_observed(bells), Some(Event::Bell)));
    // A callback may submit fresh work only after this drain has returned.
    bus.remove(bells);
    assert!(bus.take_observed(bells).is_none());
    bus.publish(Event::Bell);
    bus.drain(|_, _| {});
    assert!(bus.take_observed(other).is_none());
}
