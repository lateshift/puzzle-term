//! Statistics replay must preserve every unit and reject invalid counters.
use super::*;
/// Nonzero phase/count values prove the statistics payload survives replay intact.
pub(super) fn fixture() -> Value {
    crate::messages::stats_wire::encode(&crate::messages::FrameStats {
        cpu_ms: [0.125; 6],
        instances: [3, 5, 7],
        glyph_bytes: 1024,
        upload_bytes: 512,
        parse_bytes_per_second: 2048.0,
    })
}
/// Counts are exact and finite on native and JS targets, never rounded into range.
#[test]
fn frame_statistics_reject_invalid_counters_and_phase_lengths() {
    let registry = ExtensionRegistry::default();
    for value in [f64::NAN, f64::INFINITY, -1.0, 0.5, 9_007_199_254_740_992.0] {
        let Value::Map(mut map) = fixture() else {
            unreachable!()
        };
        map.insert("glyphBytes".into(), Value::Number(value));
        assert!(Event::from_value("render/frameRendered", &Value::Map(map), &registry).is_err());
    }
    for phases in [
        vec![],
        vec![Value::Number(1.0); 5],
        vec![Value::Number(1.0); 7],
    ] {
        let Value::Map(mut map) = fixture() else {
            unreachable!()
        };
        map.insert("cpuMs".into(), Value::List(phases));
        assert!(Event::from_value("render/frameRendered", &Value::Map(map), &registry).is_err());
    }
}
