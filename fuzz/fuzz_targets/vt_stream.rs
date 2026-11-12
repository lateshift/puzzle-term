//! Full registered VT stream, split across arbitrary transport boundaries.
//! Each input is independent; a failing corpus file fully reproduces its owner state.
//! No host window, PTY process or previous iteration can affect this result.
#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let mut session = puzzle_fuzz::new_session();
    // Vary chunk boundaries without removing any byte from the wire corpus.
    // The selector changes split positions but remains in the parsed bytes,
    // preserving malformed introducers and truncated UTF-8 in the reproducer.
    let chunk = 1 + usize::from(data.first().copied().unwrap_or_default());
    for bytes in data.chunks(chunk) {
        // drive checks ordinary wide/cluster ownership and rectangular storage
        // after each accepted feed, including both inactive and retained screens.
        puzzle_fuzz::drive(&mut session, bytes);
    }
});
