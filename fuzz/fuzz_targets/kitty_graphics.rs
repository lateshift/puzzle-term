//! APC chunking and bounded decoded storage through the real Session barrier.
//! Each input is independent; a failing corpus file fully reproduces its owner state.
//! No host window, PTY process or previous iteration can affect this result.
#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    // Admission and native completion use the same owner message barrier;
    // the harness cannot bypass validation by installing pixels into the store.
    let mut session = puzzle_fuzz::new_session();
    // Start with the production fixture's tiny source so mutations can reach
    // put/delete/replace paths even when later input contains no transmission.
    puzzle_fuzz::drive(&mut session, b"\x1b_Ga=T,f=32,s=1,v=1,i=1;/wAA/w==\x1b\\");
    // Deliberately split every byte: APC/base64 state must survive read boundaries.
    for byte in data {
        puzzle_fuzz::drive(&mut session, std::slice::from_ref(byte));
    }
});
