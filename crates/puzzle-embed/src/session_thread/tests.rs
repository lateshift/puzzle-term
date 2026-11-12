//! D-50 input wake, three-slot ownership and EOF order through the host endpoints.
use super::*;
use puzzle_term::{
    Origin, SessionConfig, SessionMessage,
    messages::Command,
    platform::{PtySize, Services, Transport, Waker},
};
use std::{io::Cursor, time::Duration};

/// Full-copy timing and flood request distributions retain meaningful acceptance evidence.
mod capture;
/// Byte-capped sparse history remains general across its first recycle boundary.
#[cfg(feature = "lock-metrics")]
mod growth;
/// Bounded App image decoding remains ordered across independent Sessions.
mod images;
/// Native held-exit lifetime and change-only event delivery regressions.
mod lifecycle;
/// Pool-fed priority controls and real-slave reply order preserve D-29's surviving guarantees.
mod pipeline;
/// A real non-reading child cannot turn query replies into an unbounded queue.
#[cfg(unix)]
mod reply_flood;
/// Release cohorts preserve every real-PTY stage trial and storage-capacity witness.
#[cfg(feature = "lock-metrics")]
mod stages;
/// A waiting renderer gets the next completed synchronized transaction.
mod sync_boundary;

/// Idle replies preserve one-time drawing registrations and their presented targets.
mod overlays;

/// Observable bytes permit proving policy execution while the drain remains idle.
struct Writer(mpsc::Sender<Vec<u8>>);
impl Transport for Writer {
    /// Preserve producer transactions exactly, as the real ordered PTY writer does.
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.0.send(bytes.to_vec()).map_err(|e| e.to_string())
    }
    /// This fixture exercises wake and byte order rather than OS resize.
    fn resize(&mut self, _: PtySize) -> Result<(), String> {
        Ok(())
    }
    /// Finite ownership is ended by dropping the drain endpoint.
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}
/// Display opportunities are explicit requests in this fixture.
struct Quiet;
impl Waker for Quiet {
    /// No event loop is necessary to observe the value channels.
    fn request_frame(&mut self) {}
    /// The fixture does not animate or depend on real cursor timers.
    fn wake_at(&mut self, _: Instant) {}
}

/// Reads an exact writer transaction with a finite deadlock-detection deadline.
fn transaction(receiver: &Receiver<Vec<u8>>) -> Vec<u8> {
    receiver
        .recv_timeout(crate::test_support::HANG_GUARD)
        .unwrap_or_else(|error| panic!("writer transaction missing: {error}"))
}

/// Idle control wake and drain EOF remain valid with cloned control senders alive.
#[test]
fn idle_input_wakes_owner_and_eof_preserves_final_reply() {
    let (writes, received) = mpsc::channel();
    let session = Session::new(
        SessionConfig::default(),
        Services::new(Box::new(Writer(writes)), Box::new(Quiet)),
    );
    let (mut producer, consumer) = crate::pty_pool::pair();
    let started = start(session, consumer, Config::default())
        .unwrap_or_else(|error| panic!("owner starts: {error}"));
    started
        .handle
        .send(SessionMessage::Command(
            Command::Text("key".into()),
            Origin::User,
        ))
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(transaction(&received), b"key");
    // Replies let the sender race the owner's transition back to idle. A wake
    // consumed during data polling must not strand the next key behind a read.
    for _ in 0..256 {
        started
            .handle
            .send(SessionMessage::Command(
                Command::Text("x".into()),
                Origin::User,
            ))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(transaction(&received), b"x");
    }
    // DA reaches the same writer after the control transaction, before drained EOF.
    assert!(
        producer
            .read_once(&mut Cursor::new(b"\x1b[c"))
            .unwrap_or_else(|error| panic!("{error:?}"))
    );
    drop(producer);
    assert!(transaction(&received).starts_with(b"\x1b[?"));
    assert!(
        started
            .worker
            .join()
            .unwrap_or_else(|error| panic!("{error:?}"))
            .is_ok()
    );
    assert_eq!(started.handle.pending_bytes(), 0);
    assert_eq!(started.handle.parsed_total(), 3);
}

/// A request cannot borrow a fourth slot while rendering owns all three answers.
#[test]
fn snapshot_pool_never_waits_for_the_renderer() {
    let (writes, received) = mpsc::channel();
    let session = Session::new(
        SessionConfig::default(),
        Services::new(Box::new(Writer(writes)), Box::new(Quiet)),
    );
    let (producer, consumer) = crate::pty_pool::pair();
    let started = start(session, consumer, Config::default())
        .unwrap_or_else(|error| panic!("owner starts: {error}"));
    let mut slots = Vec::new();
    for _ in 0..3 {
        started
            .handle
            .request_frame(
                Instant::now(),
                0,
                0,
                Instant::now() + crate::test_support::HANG_GUARD,
            )
            .unwrap_or_else(|error| panic!("{error:?}"));
        slots.push(
            started
                .frames
                .receive_until(Instant::now() + crate::test_support::HANG_GUARD)
                .unwrap_or_else(|| panic!("snapshot publication missing")),
        );
    }
    started
        .handle
        .request_frame(
            Instant::now(),
            0,
            0,
            Instant::now() + crate::test_support::HANG_GUARD,
        )
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert!(started.frames.try_receive().is_none());
    // Key policy still runs even though there is no free snapshot allocation.
    started
        .handle
        .send(SessionMessage::Command(
            Command::Text("independent".into()),
            Origin::User,
        ))
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(transaction(&received), b"independent");
    // EOF must end the finite worker without waiting for any held snapshot slot.
    drop(producer);
    assert!(
        started
            .worker
            .join()
            .unwrap_or_else(|error| panic!("{error:?}"))
            .is_ok()
    );
    assert_eq!(slots.len(), 3);
}

/// Late answers retain their sealed source and explicitly bracketed owner clocks.
mod timing;

/// Older queue entries do not postpone an on-time current publication.
mod freshness;

/// One owner translates modifiers, live settings and composition in channel order.
mod input;
