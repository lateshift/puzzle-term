//! Plain C representations carry bytes and numbers, never Rust enums as input.
//!
//! Every exported operation returns a result code. Pointer/length buffers borrow
//! caller memory only for that call; event buffers borrow library memory only
//! during a callback. Empty buffers may have null pointers. Nonempty buffers
//! require live, readable memory; no library can validate arbitrary addresses.
use std::ffi::c_void;

/// Stable ABI result values; only library outputs use this enum representation.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultCode {
    /// Operation was accepted; asynchronous effects arrive during app tick.
    Ok = 0,
    /// Null, invalid text/geometry or an oversized memory range was rejected.
    InvalidArgument = 1,
    /// Unknown, freed or wrong-kind generation-checked identity.
    InvalidHandle = 2,
    /// Calls must originate on the App's creating host thread.
    WrongThread = 3,
    /// The checked Surface is already closed.
    Closed = 4,
    /// Native initialization or an OS operation failed; tracing retains details.
    Platform = 5,
    /// An optional capability was not linked into this library build.
    Unsupported = 6,
    /// Recursive app tick or creation during an active lifecycle operation.
    Busy = 7,
    /// Identity storage is exhausted without reusing an old incarnation.
    Exhausted = 8,
}
/// A borrowed byte range, including embedded NUL; lengths are never C strings.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Bytes {
    /// Null is accepted only when len is zero.
    pub data: *const u8,
    /// Number of readable bytes, at most isize::MAX.
    pub len: usize,
}
/// Worker wakeups are notifications only; host work runs after calling app tick.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Callbacks {
    /// Passed back unchanged; the host keeps it alive until app free returns.
    pub userdata: *mut c_void,
    /// Required nonblocking, thread-safe wakeup; must not reenter Puzzle.
    pub wakeup: Option<extern "C" fn(*mut c_void)>,
    /// Optional host-thread delivery; payload memory lasts for this callback only.
    pub event: Option<extern "C" fn(*mut c_void, u64, *const Event)>,
}
/// Cold App configuration uses the same native JSON settings adapter as Rust hosts.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AppOptions {
    /// Must equal 1, rejecting incompatible structure layouts before use.
    pub abi_version: u32,
    /// Empty selects the normal native config path; otherwise UTF-8 path bytes.
    pub config_path: Bytes,
    /// Worker wakeup and main-thread event delivery contracts.
    pub callbacks: Callbacks,
}
/// Device geometry is copied; a native object is never queried on the render actor.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Geometry {
    /// Backing width in device pixels; zero is minimized.
    pub width: u32,
    /// Backing height in device pixels.
    pub height: u32,
    /// Device pixels per logical pixel; finite and positive.
    pub scale: f64,
    /// Native display identity, macOS CGDirectDisplayID on the harness.
    pub display: u64,
    /// Advertised refresh rate in millihertz.
    pub millihertz: u32,
}
/// Host UI rectangle in view-local top-left logical points, not device pixels.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LogicalRect {
    /// Logical left edge; negative positions are valid outside the surface.
    pub x: f64,
    /// Logical top edge, matching cursorRect's orientation.
    pub y: f64,
    /// Nonnegative logical width.
    pub width: f64,
    /// Nonnegative logical height.
    pub height: f64,
}
/// Native attachment and transport choice, fixed for one Surface lifetime.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SurfaceOptions {
    /// 1 AppKit NSView; 2 HWND; 3 Xlib Window; 4 Wayland surface; 5 CAMetalLayer.
    pub native_kind: u32,
    /// Native view/window; must remain alive until synchronous surface free or app free returns.
    pub native_view: *mut c_void,
    /// Xlib Display or Wayland wl_display; unused for AppKit/Win32.
    pub native_display: *mut c_void,
    /// Xlib screen index; ignored by other native kinds.
    pub native_screen: i32,
    /// 0 Puzzle-owned PTY (default), 1 host byte source.
    pub transport: u32,
    /// UTF-8 executable path; empty selects the normal login shell (PTY only).
    pub shell: Bytes,
    /// UTF-8 cwd; empty inherits the host directory (PTY only).
    pub cwd: Bytes,
    /// Initial native backing geometry and display identity.
    pub geometry: Geometry,
}
/// Callback event names use stable bus type strings; data is JSON except transport bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Event {
    /// UTF-8 name, e.g. terminal/titleChanged, clipboard/read or transport/input.
    pub name: Bytes,
    /// UTF-8 JSON for structured events; raw bytes for transport/input.
    pub payload: Bytes,
}

impl From<puzzle_embed::ApiError> for ResultCode {
    /// Preserve host misuse distinctions; diagnostics retain the platform reason.
    fn from(error: puzzle_embed::ApiError) -> Self {
        match error {
            puzzle_embed::ApiError::WrongThread => Self::WrongThread,
            puzzle_embed::ApiError::Closed => Self::Closed,
            puzzle_embed::ApiError::InvalidArgument(_) => Self::InvalidArgument,
            puzzle_embed::ApiError::Platform(error) => {
                tracing::error!(event = "ffi.platform_failed", %error);
                Self::Platform
            }
        }
    }
}

/// Native key identities mirror the safe RawInput adapter, including Option sides.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Key {
    /// 0 character, 1 named W3C key, 2 dead, 3 unidentified.
    pub kind: u32,
    /// Layout-resolved key string.
    pub logical: Bytes,
    /// Base layout character without Shift/Option.
    pub base: Bytes,
    /// W3C physical code such as KeyA.
    pub physical: Bytes,
    /// Committed text, empty if absent.
    pub text: Bytes,
    /// 0 standard, 1 left, 2 right, 3 numpad.
    pub location: u32,
    /// Low four modifier bits, plus left/right Option at 16/32.
    pub modifiers: u32,
    /// Exactly 0 release or 1 press.
    pub pressed: u32,
    /// Exactly 0 initial or 1 repeated press.
    pub repeat: u32,
}
/// Pointer sample in view-local logical pixels; negative coordinates retain edge drags.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Pointer {
    /// 0 move, 1 press, 2 release, 3 leave.
    pub kind: u32,
    /// Xterm button identity: left 0, middle 1, right 2.
    pub button: u32,
    /// Signed view-local logical x.
    pub x: f64,
    /// Signed view-local logical y.
    pub y: f64,
    /// Reserved legacy field, ignored; initialize to zero.
    pub cell_width: u32,
    /// Reserved legacy field, ignored; initialize to zero.
    pub cell_height: u32,
    /// Shift 1, Alt 2, Control 4, Super 8.
    pub modifiers: u32,
    /// Exactly 0 or 1, including motion while dragging.
    pub held: u32,
    /// Consecutive clicks at this position.
    pub clicks: u32,
}

/// Declarative annotation; fields are interpreted only by the selected kind.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Decoration {
    /// 0 highlight, 1 underline, 2 box, 3 badge, 4 inline text, 5 SVG, 6 glyph effect.
    pub kind: u32,
    /// 0 cursor, 1 viewport rect, 2 cell marker, 3 marker range, 4 line marker, 5 zone, 6 pointer.
    pub anchor_kind: u32,
    /// First marker or OSC 133 zone identity, according to anchor_kind.
    pub start: u64,
    /// Inclusive range end marker.
    pub end: u64,
    /// Viewport logical x, y, width, height.
    pub rect: [f32; 4],
    /// Z-slot 0 background through 6 top.
    pub slot: u32,
    /// Exactly 0 or 1; source rewriting/eviction removes auto declarations.
    pub auto_remove: u32,
    /// Straight packed RGBA8, 0xRRGGBBAA.
    pub color: u32,
    /// Logical corner radius for boxes.
    pub radius: f32,
    /// Logical inline-text size.
    pub size: f32,
    /// Underline 0 none/1 single/2 double/3 curly/4 dotted/5 dashed; text style 0..3.
    pub style: u32,
    /// Inline placement: 0 after, 1 before, 2 above, 3 below, 4 end of line.
    pub placement: u32,
    /// SVG fit: 0 cell, 1 range, 2 explicit transform.
    pub fit: u32,
    /// Affine coefficients a,b,c,d,e,f for SVG custom fit or glyph effects.
    pub transform: [f64; 6],
    /// Glyph-effect opacity; finite, core clamps to 0..1.
    pub opacity: f32,
    /// Exactly 0 unchanged ink color or 1 replace with color.
    pub replace_color: u32,
    /// Exactly 0 normal ink or 1 hidden ink.
    pub hidden: u32,
    /// Label/text or SVG path; bounded by the common core declaration validator.
    pub text: Bytes,
    /// Empty means no region; otherwise bounded hover tooltip text.
    pub tooltip: Bytes,
}

/// One clipboard representation; bytes are copied before a completion returns.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MimeItem {
    /// Nonempty UTF-8 MIME name, at most 256 ASCII graphic bytes.
    pub mime: Bytes,
    /// Binary data, aggregate at most 64 MiB over all representations.
    pub data: Bytes,
}

// Optional payloads are an independent whole-file header boundary.
include!("abi/optional_items.rs");
