//! Atomic rectangular text under edits, scrolling, reflow, switches and reset.
//! Each input is independent; a failing corpus file fully reproduces its owner state.
//! No host window, PTY process or previous iteration can affect this result.
#![no_main]
// Structured operations deliberately reach valid reservations and edits;
// vt_stream complements them with malformed syntax and arbitrary raw framing.
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    // The shared operation boundary checks ordinary grid ownership as well as
    // rectangular fragments; an edit defect cannot hide behind valid block keys.
    puzzle_fuzz::text_sizing(data);
});
