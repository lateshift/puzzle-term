//! Clipboard streaming is tested against wire semantics, independently of host I/O.
#![allow(
    clippy::expect_used,
    reason = "Tests report invalid protocol fixtures directly"
)]
use super::*;
/// Uses the actual bounded VT parser, including ordinary output between packets.
fn terminal(bytes: &[u8]) -> Terminal {
    let mut term = Terminal::new(80, 24, 10, 100_000);
    term.advance(bytes);
    term
}
/// Extracts a complete request; an incomplete write must never reach this event.
fn request(term: &Terminal) -> &Request {
    term.events
        .iter()
        .find_map(|event| match event {
            TerminalEvent::ClipboardRequest(request) => Some(request),
            _ => None,
        })
        .expect("complete request")
}
#[test]
fn arbitrary_packet_boundaries_and_output_preserve_binary_data() {
    let term = terminal(b"\x1b]5522;type=write:id=a!b:loc=primary\x1b\\hello\x1b]5522;type=wdata:mime=aW1hZ2UvcG5n;iV\x1b\\\x1b]5522;type=wdata:mime=aW1hZ2UvcG5n;BORw==\x1b\\\x1b]5522;type=wdata\x1b\\");
    assert_eq!(
        request(&term),
        &Request {
            id: "ab".into(),
            primary: true,
            password: None,
            operation: Operation::Write(vec![MimeData {
                mime: "image/png".into(),
                data: (&b"\x89PNG"[..]).into()
            }])
        }
    );
    assert_eq!(
        term.screen()
            .row(0)
            .expect("row")
            .cells
            .get(0)
            .expect("cell")
            .ch,
        'h' as u32
    );
    assert!(
        term.replies.is_empty(),
        "ack belongs to the host completion"
    );
}
#[test]
fn invalid_stream_aborts_until_new_start() {
    let mut term = terminal(b"\x1b]5522;type=write:id=x\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;YQ==Yg==\x1b\\\x1b]5522;type=wdata\x1b\\");
    assert_eq!(
        term.replies,
        b"\x1b]5522;type=write:status=EINVAL:id=x\x1b\\"
    );
    assert!(term.events.is_empty());
    term.advance(b"\x1b]5522;type=write\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;YQ==\x1b\\\x1b]5522;type=wdata\x1b\\");
    assert!(matches!(request(&term).operation, Operation::Write(_)));
}
#[test]
fn aliases_share_storage_and_ris_aborts_partial_data() {
    let mut term = terminal(b"\x1b]5522;type=write\x1b\\\x1b]5522;type=walias:mime=dGV4dC9wbGFpbg==;dGV4dC94LWN1c3RvbQ==\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;YQ==\x1b\\\x1b]5522;type=wdata\x1b\\");
    let Operation::Write(items) = &request(&term).operation else {
        panic!("write");
    };
    assert_eq!(items.len(), 2);
    assert!(std::sync::Arc::ptr_eq(&items[0].data, &items[1].data));
    term.advance(b"\x1b]5522;type=write\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;Y\x1b\\\x1bc\x1b]5522;type=wdata\x1b\\");
    assert!(term.events.is_empty());
}
#[test]
fn decoded_limit_is_checked_before_append() {
    let mut write = Write {
        total: crate::clipboard::MAX_BYTES - 1,
        ..Default::default()
    };
    assert_eq!(base64::feed(&mut write, b"YWI="), Err(Status::TooLarge));
    assert!(write.bytes.is_empty());
    // This is a deterministic size-bound check, independent of allocating 64 MiB.
}
#[test]
fn reads_validate_payload_and_reply_chunks_are_independently_padded() {
    let term = terminal(b"\x1b]5522;type=read:id=ok;dGV4dC9wbGFpbiBpbWFnZS9wbmc=\x1b\\");
    let req = request(&term);
    assert_eq!(
        req.operation,
        Operation::Read(vec!["text/plain".into(), "image/png".into()])
    );
    let mut out = Vec::new();
    respond(
        req,
        Status::Done,
        &[MimeData {
            mime: "image/png".into(),
            data: vec![7; 4097].into(),
        }],
        &mut out,
    );
    assert_eq!(
        out.windows(11)
            .filter(|part| *part == b"status=DATA")
            .count(),
        2
    );
    assert!(out.ends_with(b"\x1b]5522;type=read:status=DONE:id=ok\x1b\\"));
    let invalid = terminal(b"\x1b]5522;type=read;YQ\x1b\\after");
    assert!(invalid.events.is_empty());
}

#[test]
fn clipboard_raw_controls_are_rejected_instead_of_silently_repaired() {
    // vte normally strips these bytes from OSC; kitty explicitly prohibits repairing them.
    for control in [0, 1, 9, 10, 13, 31] {
        let mut bytes =
            b"\x1b]5522;type=write\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;aG".to_vec();
        bytes.push(control);
        bytes.extend_from_slice(b"k=\x1b\\\x1b]5522;type=wdata\x1b\\after");
        for split in 0..=bytes.len() {
            let mut term = Terminal::new(20, 2, 0, 0);
            term.advance(&bytes[..split]);
            term.advance(&bytes[split..]);
            assert!(term.events.is_empty());
            assert_eq!(term.replies, b"\x1b]5522;type=write:status=EINVAL\x1b\\");
        }
        let mut term = Terminal::new(20, 2, 0, 0);
        let mut read = b"\x1b]5522;type=read;dGV4".to_vec();
        read.push(control);
        read.extend_from_slice(b"dC9wbGFpbg==\x1b\\");
        term.advance(&read);
        assert!(term.events.is_empty());
        assert!(term.replies.is_empty());
    }
}

#[test]
fn aliases_replace_existing_representations_and_can_chain() {
    // Alias assignment happens at commit, after data, and later aliases see earlier ones.
    let term = terminal(b"\x1b]5522;type=write\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;YQ==\x1b\\\x1b]5522;type=wdata:mime=dGV4dC94LWN1c3RvbQ==;Yg==\x1b\\\x1b]5522;type=walias:mime=dGV4dC9wbGFpbg==;dGV4dC94LWN1c3RvbQ==\x1b\\\x1b]5522;type=walias:mime=dGV4dC94LWN1c3RvbQ==;dGV4dC9odG1s\x1b\\\x1b]5522;type=wdata\x1b\\");
    let Operation::Write(items) = &request(&term).operation else {
        panic!("write");
    };
    assert_eq!(items.len(), 3);
    for item in items {
        assert_eq!(&*item.data, b"a");
        assert!(std::sync::Arc::ptr_eq(&items[0].data, &item.data));
    }
}
