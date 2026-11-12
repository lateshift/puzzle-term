//! Session-order regressions pin the asynchronous decode boundary against VT and input.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests assert protocol outcomes and fixture ownership"
)]
use super::*;
use crate::{
    Origin, SessionConfig, SessionMessage,
    messages::Command,
    platform::{PtySize, Services, Transport, Waker},
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

/// A recording endpoint observes actual producer order, not the VT reply buffer alone.
struct Writer(Arc<Mutex<Vec<Vec<u8>>>>);
impl Transport for Writer {
    /// Each write preserves the application's request order.
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.0.lock().unwrap().push(bytes.to_vec());
        Ok(())
    }
    /// Geometry is irrelevant to this ordering witness.
    fn resize(&mut self, _: PtySize) -> Result<(), String> {
        Ok(())
    }
    /// No native lifetime exists in the pure core test.
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}
/// Frame scheduling is explicit and has no real timer in this test.
struct Quiet;
impl Waker for Quiet {
    /// Tests inspect source state rather than presenting a native frame.
    fn request_frame(&mut self) {}
    /// Injected time must never start a host timer.
    fn wake_at(&mut self, _: Instant) {}
}

/// A PNG/raw request cannot let a later DA reply overtake its worker completion.
#[test]
fn decode_pauses_output_but_input_stays_immediate_and_replies_ordered() {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let mut session = Session::new(
        SessionConfig::default(),
        Services::new(Box::new(Writer(writes.clone())), Box::new(Quiet)),
    );
    session.enable_images(4096);
    let now = Instant::now();
    let bytes = b"\x1b_Gi=7,s=1,v=1;/wAA/w==\x1b\\\x1b[cZ";
    session.feed(bytes, now);
    assert!(session.image_decode_pending());
    assert_eq!(session.terminal().cursor.column, 0);
    assert!(writes.lock().unwrap().is_empty());
    session
        .apply(
            SessionMessage::Command(Command::Text("key".into()), Origin::Api),
            now,
        )
        .unwrap();
    assert_eq!(*writes.lock().unwrap(), [b"key".to_vec()]);
    let request = session.take_image_decode_request().unwrap();
    session
        .apply(
            SessionMessage::ImageDecoded(DecodeResult {
                ticket: request.ticket,
                image: Ok(crate::images::DecodedImage {
                    size: [1, 1],
                    rgba: Arc::from(request.payload),
                }),
            }),
            now,
        )
        .unwrap();
    session.feed(&[], now);
    assert_eq!(
        *writes.lock().unwrap(),
        [
            b"key".to_vec(),
            b"\x1b_Gi=7;OK\x1b\\".to_vec(),
            b"\x1b[?62;22c".to_vec()
        ]
    );
    assert_eq!(session.terminal().cursor.column, 1);
}

/// Native opt-in leaves the warmed text-only path allocation-free.
#[test]
fn enabled_images_without_graphics_allocate_nothing_when_warm() {
    let mut session = Session::new(
        SessionConfig {
            columns: 20,
            rows: 4,
            scrollback_lines: 0,
            parse_budget: Duration::from_secs(1),
            ..SessionConfig::default()
        },
        Services::new(
            Box::new(Writer(Arc::new(Mutex::new(Vec::new())))),
            Box::new(Quiet),
        ),
    );
    session.enable_images(4096);
    let now = Instant::now();
    for _ in 0..100 {
        session.feed(b"hello world\r\n", now);
    }
    let measured = allocation_counter::measure(|| {
        for _ in 0..100 {
            session.feed(b"hello world\r\n", now);
        }
    });
    assert_eq!(measured.count_total, 0);
}
