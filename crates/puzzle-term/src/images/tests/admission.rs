//! Admission and the raster executor share exact tiled border accounting.
use super::*;

/// A decoded-exact-fit source must reject before OK if hardware borders exceed quota.
#[test]
fn hardware_borders_are_charged_before_protocol_success() {
    let mut terminal = Terminal::new(10, 4, 0, 0);
    let mut store = Store::new(1024 * 1024, &terminal);
    store.set_texture_limit(8192, &mut terminal);
    let size = [16384, 16];
    let decoded = pixel_bytes(size, 4).unwrap();
    let charge = puzzle_frame::image_resident_bytes(size, 8192);
    assert_eq!(decoded, 1_048_576);
    assert_eq!(charge, 1_048_832);
    // Raw dimensions are known before dispatch to the bounded worker.
    store.command(b"i=3,f=32,s=16384,v=16;AAAA", &mut terminal);
    assert!(!store.decode_pending());
    assert_eq!(
        terminal.replies,
        b"\x1b_Gi=3;ENOSPC:image storage limit exceeded\x1b\\"
    );
    terminal.replies.clear();
    // PNG dimensions become authoritative only after decode. The same footprint
    // check must run before committing any source or acknowledging success.
    store.command(b"i=3,f=100;AAAA", &mut terminal);
    let request = store.take_request().unwrap();
    let image = DecodedImage {
        size,
        rgba: vec![0; decoded].into(),
    };
    store.complete(
        DecodeResult {
            ticket: request.ticket,
            image: Ok(image.clone()),
        },
        &mut terminal,
    );
    assert_eq!(
        terminal.replies,
        b"\x1b_Gi=3;ENOSPC:image storage limit exceeded\x1b\\"
    );
    assert!(store.images().is_empty());
    // Raising only the test allowance to the exact footprint demonstrates there
    // is no arbitrary size cutoff and decoded/GPU accounting stay independent.
    terminal.replies.clear();
    store.set_limit(charge, &mut terminal);
    store.command(b"i=3,f=100;AAAA", &mut terminal);
    let request = store.take_request().unwrap();
    store.complete(
        DecodeResult {
            ticket: request.ticket,
            image: Ok(image),
        },
        &mut terminal,
    );
    assert_eq!(terminal.replies, b"\x1b_Gi=3;OK\x1b\\");
    assert_eq!(store.decoded_bytes(), decoded);
    assert_eq!(store.charged, charge);
    store.set_limit(decoded, &mut terminal);
    assert!(store.images().is_empty());
    assert_eq!(store.charged, 0);
}

/// ENOSPC preserves every visible source and does not partially evict hidden candidates.
#[test]
fn admission_never_displaces_other_visible_images() {
    let mut terminal = Terminal::new(10, 4, 0, 0);
    let mut store = Store::new(4096, &terminal);
    transmit(&mut store, &mut terminal, b"a=T,i=1,C=1,s=1,v=1;/wAA/w==");
    store.set_limit(4, &mut terminal);
    let generation = store.images()[0].generation;
    terminal.replies.clear();
    transmit(&mut store, &mut terminal, b"a=T,i=2,C=1,s=1,v=1;/wAA/w==");
    assert_eq!(
        terminal.replies,
        b"\x1b_Gi=2;ENOSPC:image storage limit exceeded\x1b\\"
    );
    assert_eq!(store.images()[0].generation, generation);
    // Explicit ID replacement has its own old generation's credit, even visible.
    transmit(&mut store, &mut terminal, b"i=1,s=1,v=1;/wAA/w==");
    assert_ne!(store.images()[0].generation, generation);
    assert_eq!(store.charged, 4);
    // Without a visible placement, that same image is an eligible LRU victim.
    store.command(b"a=d,d=a", &mut terminal);
    transmit(&mut store, &mut terminal, b"i=2,s=1,v=1;/wAA/w==");
    assert_eq!(store.images()[0].id, 2);
    assert_eq!(store.decoded_bytes(), 4);
}
