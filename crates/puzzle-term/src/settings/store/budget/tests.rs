//! Configuration clamping reports every input but logs its structural floor once.
use super::*;
use crate::settings::Layer;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// Captures only this test thread, without installing a global subscriber.
struct Warnings(Arc<AtomicUsize>);
impl tracing::Subscriber for Warnings {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() == tracing::Level::WARN {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// A config reload and its transactional clone must not repeat the warning.
#[test]
fn structural_clamp_logs_once_across_reload() {
    let count = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::with_default(Warnings(count.clone()), || {
        let mut store = SettingsStore::default();
        for _ in 0..3 {
            let report = store.apply_key(
                "render.glyphMemoryMb",
                puzzle_bus::Value::Number(0.0),
                Layer::User,
            );
            assert_eq!(report.clamped, ["render.glyphMemoryMb"]);
            let patch = puzzle_bus::Value::Map(std::collections::BTreeMap::from([(
                "render.glyphMemoryMb".into(),
                puzzle_bus::Value::Number(1.0),
            )]));
            assert_eq!(store.replace_user(&patch).clamped, ["render.glyphMemoryMb"]);
        }
    });
    assert_eq!(count.load(Ordering::Relaxed), 1);
}
