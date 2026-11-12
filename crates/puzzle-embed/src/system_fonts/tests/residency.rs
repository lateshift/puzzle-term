//! Capacity retirement must release mappings without losing future covered ink.
use super::*;
use puzzle_term::text::cluster_source::ClusterGlyph;

/// Alternating scripts under one loaded-face slot must reopen the evicted source correctly.
#[test]
fn retired_face_reopens_with_identical_ink() {
    let mut source = NativeFonts::new(vec!["Heiti SC".into()], false, true, metrics());
    source.ensure_index();
    source.resident = resident::Resident::bounded(1, 256 * 1024 * 1024);
    source.timing = Some(std::sync::Arc::new(timing::Timing::default()));
    let first = outline(&mut source, "漢");
    source.families = vec!["Apple SD Gothic Neo".into()];
    let korean = outline(&mut source, "한");
    assert!(!korean.is_empty());
    source.families = vec!["Heiti SC".into()];
    assert_eq!(
        outline(&mut source, "漢"),
        first,
        "retired mapping changed ink"
    );
    let report = source.timing.as_ref().unwrap().report();
    assert_eq!(report["mapping"]["count"], 3);
    assert_eq!(report["loaded_faces"]["peak_faces"], 1);
    assert_eq!(report["loaded_faces"]["evictions"], 2);
    assert!(report["loaded_faces"]["mapped_bytes"].as_u64().unwrap() <= 256 * 1024 * 1024);
}

/// Real TTC styles must share one mapping while retaining distinct face ink.
#[test]
fn collection_faces_share_one_mapping() {
    let mut source = NativeFonts::new(vec!["Heiti SC".into()], false, true, metrics());
    source.ensure_index();
    source.timing = Some(std::sync::Arc::new(timing::Timing::default()));
    for style in [FontStyle::Regular, FontStyle::Bold] {
        assert!(
            source
                .resolve(
                    ClusterRequest {
                        cluster: "漢",
                        style,
                        presentation: Presentation::Text,
                    },
                    &source.probe_placement()
                )
                .is_some()
        );
    }
    let report = source.timing.as_ref().unwrap().report();
    assert_eq!(
        report["mapping"]["count"], 1,
        "TTC face remapped the same file"
    );
}

/// Owned ink can be compared after its original file owner has retired.
fn outline(source: &mut NativeFonts, cluster: &str) -> puzzle_term::text::geometry::BezPath {
    let ink = source
        .resolve(
            ClusterRequest {
                cluster,
                style: FontStyle::Regular,
                presentation: Presentation::Text,
            },
            &source.probe_placement(),
        )
        .unwrap();
    match ink {
        ClusterGlyph::Outline(path) | ClusterGlyph::StyledOutline { path, .. } => path,
        _ => panic!("installed outline source returned no outline"),
    }
}
