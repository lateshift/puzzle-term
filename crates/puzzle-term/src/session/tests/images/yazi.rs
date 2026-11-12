//! Yazi 26.9.1 KgpOld sends quiet one-cell crops, not virtual placeholders.
use super::*;

/// A populated full screen exceeds common fixed placement caps without losing tiles.
#[test]
fn yazi_kgp_old_many_quiet_crops_and_selective_deletion() {
    let (mut host, _, _) = fixture();
    let mut context = super::super::overlays::text_context(2.0);
    host.set_text_context(&context);
    host.resize_grid(120, 50, [2400, 2400]).expect("grid");
    host.enable_images(8 * 1024 * 1024);
    let now = Instant::now();
    host.feed(
        b"\x1b[?25l\x1b_Gq=2,a=t,i=42,f=24,s=576,v=631,m=1;AAAA\x1b\\\x1b_Gm=0;AAAA\x1b\\",
        now,
    );
    let request = host.take_image_decode_request().expect("decode");
    host.apply(
        SessionMessage::ImageDecoded(DecodeResult {
            ticket: request.ticket,
            image: Ok(DecodedImage {
                size: [576, 631],
                rgba: Arc::from(vec![180; 576 * 631 * 4]),
            }),
        }),
        now,
    )
    .expect("completion");
    for row in 0..50 {
        for column in 0..120 {
            let id = row * 120 + column + 1;
            // Header/cursor order matches kgp_old.rs; q=2 makes failures silent on wire.
            host.feed(format!("\x1b[{};{}H\x1b_Gq=2,a=p,i=42,p={id},x=18,y=40,w=18,h=40,c=1,r=1,z=-1,C=1\x1b\\",row+1,column+1).as_bytes(),now);
            assert_eq!(
                (host.terminal().cursor.row, host.terminal().cursor.column),
                (row, column)
            );
        }
    }
    assert_eq!(host.images().expect("store").placements().len(), 6000);
    let frame = host.draw(now, &mut context).expect("frame");
    assert_eq!(frame.rasters.len(), 6000);
    assert_eq!(frame.images.as_ref().expect("sources").sources.len(), 1);
    for quad in &frame.rasters {
        assert_eq!(
            quad.uv,
            [18.0 / 576.0, 40.0 / 631.0, 36.0 / 576.0, 80.0 / 631.0]
        );
    }
    host.feed(b"\x1b_Ga=d,d=i,i=42,p=17\x1b\\", now);
    let store = host.images().expect("store");
    assert_eq!(store.placements().len(), 5999);
    assert!(!store.placements().iter().any(|p| p.id == 17));
    assert_eq!(store.images().len(), 1);
    host.feed(b"\x1b_Ga=d,d=I,i=42\x1b\\", now);
    assert!(host.images().expect("store").images().is_empty());
    assert!(host.images().expect("store").placements().is_empty());
}
