//! Rockorager in-band resize notifications, mode 2048 (D-58).
//! Reference: <https://gist.github.com/rockorager/e695fb2924d36b2bcf1fff4a3704bd83>.
use crate::Terminal;

/// Emits the existing geometry report after the terminal has committed its resize.
pub(crate) fn resized(term: &mut Terminal, columns: usize, rows: usize) {
    // Opt-in mode state belongs to the terminal, independent of the wire formatter.
    if term.modes.get(true, 2048) == Some(true) {
        // Cell metrics are device pixels, matching the protocol pixel-area fields.
        let (cell_width, cell_height) = term.cell_pixels;
        // CSI 48 reports both cell and pixel geometry after a resize.
        term.replies.extend_from_slice(
            format!(
                "\x1b[48;{rows};{columns};{};{}t",
                rows.saturating_mul(usize::from(cell_height)),
                columns.saturating_mul(usize::from(cell_width))
            )
            .as_bytes(),
        );
    }
}
