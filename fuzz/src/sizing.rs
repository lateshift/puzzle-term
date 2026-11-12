//! Structured OSC 66 fuzzing reaches valid rectangles and hostile edit mixtures.
//! Raw byte fuzzing remains a separate target; here mutation chooses an operation
//! from the existing M18.7 proptest family and checks both screens after each one.
use arbitrary::Unstructured;
use puzzle_term::vt::Terminal;

/// Replays a bounded list of independent sizing/edit/scroll/resize operations.
/// Returns the number of operations, bounded by the input byte count.
pub fn text_sizing(data: &[u8]) -> usize {
    let mut input = Unstructured::new(data);
    // Tiny geometry makes wrap, history eviction and rectangle clipping reachable
    // after few mutations, while every production parser rule stays unchanged.
    let mut term = Terminal::new(12, 4, crate::HISTORY_LINES, crate::HISTORY_BYTES);
    let mut operations = 0;
    // Primitive Arbitrary reads pad exhausted input with zero and may still
    // return Ok. Exhaustion, not its Result, must terminate this operation list.
    while !input.is_empty() {
        let op = input.arbitrary::<u8>().unwrap_or_default();
        operations += 1;
        // Reading each payload before execution makes the exact byte file a
        // sufficient reproducer; no random seed or workstation clock is used.
        match op % 8 {
            // Valid wire metadata covers scale/width/fraction/alignment together.
            0 => sized(&mut term, &mut input),
            // Ordinary printing interleaves clusters and scalar overwrites.
            1 => {
                let scalar = input.arbitrary::<char>().unwrap_or('x');
                term.advance(scalar.to_string().as_bytes());
            }
            // Every editing family can intersect a rectangular reservation.
            2 => {
                let commands = [
                    "\x1b[@", "\x1b[P", "\x1b[X", "\x1b[K", "\x1b[J", "\x1b[L", "\x1b[M",
                ];
                let index = input.arbitrary::<u8>().unwrap_or_default() as usize % commands.len();
                term.advance(commands[index].as_bytes());
            }
            // A partial region exercises row moves without primary history.
            3 => term.advance(b"\x1b[2;4r\x1b[4;1H\n\x1b[r\r\n"),
            // Reflow must preserve or retire complete blocks atomically on both
            // shrinking and growing, including a single-row destination.
            4 => {
                let width = 2 + input.arbitrary::<u8>().unwrap_or_default() as usize % 79;
                let height = 1 + input.arbitrary::<u8>().unwrap_or_default() as usize % 24;
                term.resize(width, height);
            }
            // Switching away and back must not alias independent screen slots.
            5 => term.advance(b"\x1b[?1049hscaled\x1b[?1049l"),
            // Reset drops the entire sparse model before later operations.
            6 => term.advance(b"\x1bc"),
            // Full-width scroll and reverse scroll stress history recycling.
            _ => term.advance(b"\r\n\x1b[S\x1b[T"),
        }
        // Check immediately, before another operation can erase evidence of a
        // dangling key, missing fragment or incorrect storage-accounting delta.
        crate::invariants(&term);
    }
    operations
}

/// All six wire keys vary, including invalid fractions and explicit w=0.
fn sized(term: &mut Terminal, input: &mut Unstructured<'_>) {
    let fields = input.arbitrary::<[u8; 6]>().unwrap_or_default();
    let [s, w, n, d, v, h] = fields;
    // Restricting only to the protocol's numeric ranges reaches valid blocks;
    // raw vt_stream independently mutates signs/overflow/invalid UTF-8/framing.
    let wire = format!(
        "\x1b]66;s={}:w={}:n={}:d={}:v={}:h={};A一é\x1b\\",
        1 + s % 7,
        w % 8,
        n % 16,
        d % 16,
        v % 3,
        h % 3
    );
    term.advance(wire.as_bytes());
}

#[cfg(test)]
/// Empty and truncated seeds must produce a finite operation list.
mod tests;
