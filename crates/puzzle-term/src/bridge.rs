//! I/O-free puzzle-bridge-v1 codec (design §12.3).
//!
//! The standalone bridge compiles this source module directly; the browser
//! reaches it through puzzle-term. This shares one wire implementation without
//! making the server depend on the terminal engine or exposing native APIs.
//! Payload slices borrow the WebSocket message. Integers have fixed widths;
//! resize dimensions are validated before they can reach an OS or a grid.
//!
//! Tags identify direction as well as operation: a server packet sent by a
//! client is rejected, so output cannot be mistaken for input. Fixed-size
//! controls require their entire exact length; trailing bytes are not silently
//! interpreted as a second request. The WebSocket already supplies framing.
//! Byte payloads remain opaque because UTF-8 and VT sequences can span reads.
//! Only the human-readable error payload requires UTF-8. Hello validates both
//! its version and its two-valued appearance before a server starts a child.
//! Owned encoding buffers stay with the caller, enabling reuse on the web
//! while decoders borrow frame data without copying or allocating.
use std::fmt;

/// Transfer witnesses share the same diagnostic hash; it is not a wire credential.
#[path = "bridge/checksum.rs"]
pub mod checksum;

/// Protocol version understood by both endpoints.
pub const VERSION: u8 = 1;
/// Shared byte budgets keep both endpoints on the same flow-control protocol.
#[path = "bridge/limits.rs"]
mod limits;
pub use limits::{ACK_THRESHOLD, INPUT_WINDOW, OUTPUT_WINDOW, PAYLOAD_CHUNK};
/// Largest application message: one payload chunk plus its type prefix.
/// WebSocket frame limits use this too, bounding client input allocations.
pub const MAX_MESSAGE: usize = PAYLOAD_CHUNK + 1;

/// Physical PTY dimensions; pixel extents may be zero when unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    /// Character columns, limited to the §16 grid cap.
    pub cols: u16,
    /// Character rows, also subject to the total-cell cap.
    pub rows: u16,
    /// Width in device pixels, not CSS pixels.
    pub width_px: u16,
    /// Height in device pixels, not logical pixels.
    pub height_px: u16,
}

/// Decoded client-to-server request; hello must precede all other requests.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Client<'a> {
    /// Input is already encoded by the client's terminal modes.
    Input(&'a [u8]),
    /// Grid and device-pixel size, with validated nonzero cell dimensions.
    Resize(Size),
    /// Credit for bytes actually parsed, not merely received by the browser.
    Ack(u32),
    /// Initial appearance; false is dark, true is light.
    Hello {
        /// Light appearance selects COLORFGBG=0;15 at spawn.
        light: bool,
    },
}

/// Decoded server-to-client message; output must precede the exit message.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Server<'a> {
    /// Raw PTY output, including split UTF-8 or control sequences.
    Output(&'a [u8]),
    /// Child exit status as a signed 32-bit little-endian integer.
    Exit(i32),
    /// Server confirms the agreed protocol before output starts.
    Hello,
    /// Human-readable UTF-8 explanation, never arbitrary binary text.
    Error(&'a str),
    /// Additional input bytes permitted; replenished only after actual PTY writes.
    InputCredit(u32),
}

/// A malformed message cannot be interpreted partially.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeError;

/// Errors carry a concise explanation suitable for the bridge's error frame.
impl fmt::Display for DecodeError {
    /// Formats validation failure without disclosing host details.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid puzzle-bridge-v1 message")
    }
}
/// Enables ordinary Result propagation in callers without a new dependency.
impl std::error::Error for DecodeError {}

/// Validates cell counts before allocation or PTY resize (design §16).
impl Size {
    /// Rejects zero dimensions, either axis above 4096, or over one million cells.
    pub fn valid(self) -> bool {
        (2..=4096).contains(&self.cols)
            && (1..=4096).contains(&self.rows)
            && u32::from(self.cols) * u32::from(self.rows) <= 1_000_000
    }
}

/// Reads one fixed-width integer; checked slices reject all truncated frames.
fn integer<const N: usize>(bytes: &[u8], at: usize) -> Result<[u8; N], DecodeError> {
    bytes
        .get(at..at + N)
        .ok_or(DecodeError)?
        .try_into()
        .map_err(|_| DecodeError)
}

/// Validates direction, exact control lengths, version and appearance.
/// Unknown tags and trailing bytes in fixed-size controls are errors.
pub fn decode_client(bytes: &[u8]) -> Result<Client<'_>, DecodeError> {
    if bytes.len() > MAX_MESSAGE {
        return Err(DecodeError);
    }
    // Checking first() before dispatch also rejects an empty frame. Length
    // guards on each control make the remaining fixed offsets valid.
    match bytes.first().copied() {
        Some(0x00) => Ok(Client::Input(&bytes[1..])),
        Some(0x01) if bytes.len() == 9 => {
            // LE integers are parsed individually; no alignment or unsafe cast.
            let size = Size {
                cols: u16::from_le_bytes(integer(bytes, 1)?),
                rows: u16::from_le_bytes(integer(bytes, 3)?),
                width_px: u16::from_le_bytes(integer(bytes, 5)?),
                height_px: u16::from_le_bytes(integer(bytes, 7)?),
            };
            if size.valid() {
                Ok(Client::Resize(size))
            } else {
                Err(DecodeError)
            }
        }
        Some(0x02) if bytes.len() == 5 => Ok(Client::Ack(u32::from_le_bytes(integer(bytes, 1)?))),
        Some(0x03) if bytes.len() == 3 && bytes[1] == VERSION && bytes[2] <= 1 => {
            Ok(Client::Hello {
                light: bytes[2] == 1,
            })
        }
        _ => Err(DecodeError),
    }
}

/// Validates server messages before the web adapter dispatches any bytes.
pub fn decode_server(bytes: &[u8]) -> Result<Server<'_>, DecodeError> {
    if bytes.len() > MAX_MESSAGE {
        return Err(DecodeError);
    }
    match bytes.first().copied() {
        Some(0x80) => Ok(Server::Output(&bytes[1..])),
        Some(0x81) if bytes.len() == 5 => Ok(Server::Exit(i32::from_le_bytes(integer(bytes, 1)?))),
        // The server hello is exactly tag/version; no implicit fallback to
        // another protocol is allowed after a mismatched version byte.
        Some(0x82) if bytes == [0x82, VERSION] => Ok(Server::Hello),
        Some(0x83) => Ok(Server::Error(
            std::str::from_utf8(&bytes[1..]).map_err(|_| DecodeError)?,
        )),
        Some(0x84) if bytes.len() == 5 => {
            Ok(Server::InputCredit(u32::from_le_bytes(integer(bytes, 1)?)))
        }
        _ => Err(DecodeError),
    }
}

/// Encodes a validated client request into caller-owned reusable storage.
pub fn encode_client(message: Client<'_>, into: &mut Vec<u8>) -> Result<(), DecodeError> {
    // Failed encodings never leave an older valid message in the scratch
    // buffer, which could otherwise send stale input after a rejected resize.
    into.clear();
    match message {
        Client::Input(bytes) if bytes.len() < MAX_MESSAGE => {
            into.push(0x00);
            into.extend_from_slice(bytes);
        }
        Client::Resize(size) if size.valid() => {
            into.push(0x01);
            // Wire order is cells first, then physical pixels (§12.3).
            for value in [size.cols, size.rows, size.width_px, size.height_px] {
                into.extend_from_slice(&value.to_le_bytes());
            }
        }
        Client::Ack(bytes) => {
            into.push(0x02);
            into.extend_from_slice(&bytes.to_le_bytes());
        }
        Client::Hello { light } => into.extend_from_slice(&[0x03, VERSION, u8::from(light)]),
        _ => return Err(DecodeError),
    }
    Ok(())
}

/// Encodes output, exit or a bounded UTF-8 error without a JSON dependency.
pub fn encode_server(message: Server<'_>, into: &mut Vec<u8>) -> Result<(), DecodeError> {
    into.clear();
    match message {
        Server::Output(bytes) if bytes.len() < MAX_MESSAGE => {
            into.push(0x80);
            into.extend_from_slice(bytes);
        }
        Server::Exit(code) => {
            into.push(0x81);
            into.extend_from_slice(&code.to_le_bytes());
        }
        Server::Hello => into.extend_from_slice(&[0x82, VERSION]),
        Server::InputCredit(bytes) => {
            // Grants count payload bytes, excluding the WebSocket/type framing.
            into.push(0x84);
            into.extend_from_slice(&bytes.to_le_bytes());
        }
        Server::Error(text) if text.len() < MAX_MESSAGE => {
            into.push(0x83);
            into.extend_from_slice(text.as_bytes());
        }
        _ => return Err(DecodeError),
    }
    Ok(())
}

#[cfg(test)]
#[path = "bridge/tests.rs"]
/// Literal protocol fixtures and hostile-frame properties.
mod tests;
