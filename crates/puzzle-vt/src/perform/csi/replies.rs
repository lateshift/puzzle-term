//! Terminal identity and bounded CSI query replies (§6.12, §16).
//!
//! Replies are buffered in parser order for Session to write after each
//! feed slice. No query performs host I/O. Title disclosure (CSI 21 t) is
//! deliberately disabled; color and size reports do not reveal user text.
//! DECOM changes CPR's row and column basis, matching xterm ctlseqs.
//!
//! xterm ctlseqs defines three DA forms, distinguished by an empty, `>` or
//! `=` introducer. A program may probe all three while selecting a terminfo
//! profile, so the prefix must be part of dispatch rather than inferred
//! from the numeric parameter. The fixed DA1/DA3 values describe a VT220
//! class terminal without exposing machine identity. DA2 carries only the
//! package version, encoded as a decimal protocol value.
//!
//! DSR 5 is a readiness probe and DSR 6 asks for a one-based cursor
//! position. DECXCPR adds the page number. Under DECOM, the reported origin
//! follows the scrolling region; applications use that coordinate basis
//! when they paint within DECSTBM/DECSLRM margins. Saturating subtraction
//! keeps reports safe even if a later resize clamps the cursor first.
//!
//! XTWINOPS 14/16/18 expose text-area and cell geometry only. CSI 22/23
//! operate on the bounded title stack, whereas CSI 21 is deliberately
//! unanswered because it could leak a page or document title (§16).

use super::raw;
use crate::terminal::Terminal;
use vte::Params;

/// Dispatches identity, status or size queries by prefix and final byte.
pub(super) fn dispatch(term: &mut Terminal, params: &Params, prefix: &[u8], action: char) {
    match (prefix, action) {
        (b"", 'c') => term.replies.extend_from_slice(b"\x1b[?62;22c"),
        (b">", 'c') => {
            // DA2 is a terminal identity reply, not a screen-state report.
            let version = version_integer();
            term.replies
                .extend_from_slice(format!("\x1b[>1;{version};0c").as_bytes());
        }
        (b"=", 'c') => term.replies.extend_from_slice(b"\x1bP!|00000000\x1b\\"),
        (b">", 'q') => term.replies.extend_from_slice(
            format!("\x1bP>|puzzle({})\x1b\\", env!("CARGO_PKG_VERSION")).as_bytes(),
        ),
        (b"", 'n') if raw(params, 0) == 5 => term.replies.extend_from_slice(b"\x1b[0n"),
        (b"", 'n') if raw(params, 0) == 6 => cursor_report(term, false),
        (b"?", 'n') if raw(params, 0) == 6 => cursor_report(term, true),
        (b"?", 'n') if raw(params, 0) == 996 => crate::protocols::contour::appearance(term),
        (b"", 't') => window_operation(term, params),
        _ => {}
    }
}

/// Reports the cursor using DECOM-relative coordinates when enabled.
fn cursor_report(term: &mut Terminal, private: bool) {
    // xterm ctlseqs: origin mode changes the basis of CPR and DECXCPR.
    let origin = term.modes.get(true, 6) == Some(true);
    let row_base = if origin { term.margins.top } else { 0 };
    let column_base = if origin && term.lrmm_enabled {
        term.margins.left
    } else {
        0
    };
    let row = term.cursor.row.saturating_sub(row_base) + 1;
    // The wire format is one-based even though grid indices are zero-based.
    let column = term.cursor.column.saturating_sub(column_base) + 1;
    let reply = if private {
        format!("\x1b[?{row};{column};1R")
    } else {
        format!("\x1b[{row};{column}R")
    };
    term.replies.extend_from_slice(reply.as_bytes());
}

/// Answers only safe XTWINOPS queries and handles bounded title stack ops.
fn window_operation(term: &mut Terminal, params: &Params) {
    let (columns, rows) = term.size();
    let (cell_width, cell_height) = term.cell_pixels;
    let response = match raw(params, 0) {
        14 => Some(format!(
            "\x1b[4;{};{}t",
            rows.saturating_mul(usize::from(cell_height)),
            columns.saturating_mul(usize::from(cell_width))
        )),
        16 => Some(format!("\x1b[6;{cell_height};{cell_width}t")),
        18 => Some(format!("\x1b[8;{rows};{columns}t")),
        22 => {
            // xterm's window-operation stack retains the previous title.
            term.push_title();
            None
        }
        23 => {
            // The optional parameter selects title/icon restoration.
            term.pop_title(raw(params, 1));
            None
        }
        // CSI 21 t would disclose an untrusted title to an application.
        21 => None,
        _ => None,
    };
    if let Some(reply) = response {
        term.replies.extend_from_slice(reply.as_bytes());
    }
}

/// Encodes the crate semver as a stable DA2 decimal protocol number.
fn version_integer() -> u32 {
    // Package metadata is compile-time trusted, yet parsing defensively
    // avoids a runtime panic if the version format ever changes.
    let major = env!("CARGO_PKG_VERSION_MAJOR").parse::<u32>().unwrap_or(0);
    let minor = env!("CARGO_PKG_VERSION_MINOR").parse::<u32>().unwrap_or(0);
    let patch = env!("CARGO_PKG_VERSION_PATCH").parse::<u32>().unwrap_or(0);
    major * 10_000 + minor * 100 + patch
}
