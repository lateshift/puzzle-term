//! Cancellation bounds a silent inherited descriptor without requiring kernel EOF.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures identify IO setup or a stuck shutdown"
)]
use super::*;
use std::{
    os::unix::net::UnixStream,
    sync::atomic::AtomicBool,
    sync::{Arc, mpsc},
};

/// A descendant can keep its slave open after the shell exits. This silent socket
/// models that missing EOF while exercising the real poll and cancellation pipe.
#[test]
fn cancellation_closes_a_silent_master_without_peer_eof() {
    let (master, peer) = UnixStream::pair().unwrap();
    let (read, write) = UnixStream::pair().unwrap();
    let wake = Arc::new(Wake {
        cancelled: AtomicBool::new(false),
        pipe: write,
    });
    let cancel = Cancel {
        inner: Some(wake.clone()),
    };
    let mut reader = Reader {
        master: filedescriptor::FileDescriptor::dup(&master).unwrap(),
        pipe: read,
        wake,
        deadline: None,
    };
    drop(master);
    let (done, complete) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = reader.read(&mut [0; 1]);
        drop(reader);
        done.send(result).unwrap();
    });
    // A persistent shutdown wake works whether poll has started or not. The peer
    // deliberately stays alive until after the read, so EOF cannot mask the defect.
    cancel.stop();
    assert_eq!(
        complete
            .recv_timeout(crate::test_support::HANG_GUARD)
            .expect("cancelled poll never released")
            .unwrap(),
        0
    );
    worker.join().unwrap();
    drop(peer);
}
