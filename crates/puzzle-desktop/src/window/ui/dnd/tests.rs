//! File names never become file contents or shell syntax during a native drop.
use super::*;
#[test]
fn file_uri_encoding_and_plain_path_fallback() {
    let paths = vec![
        PathBuf::from("/tmp/a b#\".png"),
        PathBuf::from("/tmp/é.png"),
    ];
    let values = representations(&paths);
    assert_eq!(
        &*values[0].data,
        b"file:///tmp/a%20b%23%22.png\r\nfile:///tmp/%C3%A9.png\r\n"
    );
    assert_eq!(&*values[1].data, "/tmp/a b#\".png\n/tmp/é.png".as_bytes());
    assert_eq!(
        quote_path(std::path::Path::new("/tmp/a'b")),
        "'/tmp/a'\\''b' "
    );
    assert!(names().iter().all(|i| i.data.is_empty()));
}

#[test]
fn rejected_or_unavailable_drop_consumes_gesture_acceptance() {
    let mut target = Target {
        accepted: true,
        preferred: vec!["image/png".into()],
        hovered: vec![PathBuf::from("/tmp/one")],
        dropped: vec![PathBuf::from("/tmp/one")],
        ..Default::default()
    };
    // The program chose a type this file-only adapter cannot supply.
    assert!(target.capture_drop().is_none());
    assert!(!target.accepted && target.hovered.is_empty() && target.dropped.is_empty());
    // A later drop cannot inherit that earlier approval, even with available MIME types.
    target.preferred.clear();
    target.dropped.push(PathBuf::from("/tmp/two"));
    assert!(target.capture_drop().is_none());
    // Explicitly accepting the next gesture allows its path representations.
    target.accepted = true;
    target.dropped.push(PathBuf::from("/tmp/three"));
    let Some(items) = target.capture_drop() else {
        panic!("explicit approval must deliver available filename representations");
    };
    assert_eq!(items.len(), 2);
    assert!(!target.accepted);
}
