//! Native immutable image ownership, independent of ready/requested render state.
use super::*;

/// Run under the existing serial native GPU test, never a competing device fixture.
pub(super) fn verify(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<(), String> {
    let shared = mailbox();
    let mut target = pool::target(Arc::clone(&shared), 2);
    let snapshot = Snapshot::default();
    let mut bytes = [0x5a; 32];
    assert!(matches!(
        snapshot.copy(2, &mut bytes),
        Err(SnapshotError::Unsupported)
    ));
    let geometry = Geometry {
        size: [63, 31],
        scale: 2.,
    };
    let image = target
        .acquire(device, geometry)?
        .ok_or("no native snapshot image")?;
    draw_red(device, queue, &image);
    target.submitted(image, queue, 1)?;
    let ready = mailbox::take_ready(
        &shared,
        geometry,
        Some(Instant::now() + Duration::from_secs(120)),
    )
    .0
    .ok_or("native snapshot completion hang")?;
    // A ready completion alone is insufficient: this is not yet the displayed front.
    assert!(matches!(
        snapshot.copy(2, &mut bytes),
        Err(SnapshotError::Unsupported)
    ));
    snapshot.presented(Arc::clone(&ready.slot), geometry);
    drop(ready);
    assert!(matches!(
        snapshot.copy(2, &mut []),
        Err(SnapshotError::BufferTooSmall([2, 1]))
    ));
    assert_eq!(
        snapshot.copy(2, &mut bytes).map_err(|e| format!("{e:?}"))?,
        [2, 1]
    );
    assert_eq!(&bytes[..8], &[255, 0, 0, 255, 255, 0, 0, 255]);
    assert!(bytes[8..].iter().all(|byte| *byte == 0x5a));
    // An outstanding geometry and another completed frame never replace the front.
    let next = Geometry {
        size: [80, 40],
        scale: 1.,
    };
    let image = target
        .acquire(device, next)?
        .ok_or("no spare native snapshot image")?;
    target.submitted(image, queue, 2)?;
    wait_native_completion(&shared, 2)?;
    assert_eq!(
        snapshot.copy(2, &mut bytes).map_err(|e| format!("{e:?}"))?,
        [2, 1]
    );
    assert_eq!(&bytes[..8], &[255, 0, 0, 255, 255, 0, 0, 255]);
    snapshot.clear();
    assert!(matches!(
        snapshot.copy(2, &mut bytes),
        Err(SnapshotError::Unsupported)
    ));
    Ok(())
}
