//! Host service contracts around the platform-neutral session (§4.3).
//!
//! A browser callback, desktop PTY and recording test transport all write
//! the same byte slice. The terminal core itself owns neither I/O nor time.
//! `Waker` schedules presentation and timers; asking it for a frame never
//! encodes input, so keys can reach the shell before the next refresh.
//!
//! The clock measures only a feed call's work. Injecting it lets a test
//! advance time after a 64 KiB slice without sleeping or depending on CPU
//! speed. `Instant` remains the public clock type on native and wasm.
//! Clipboard, URL and notification traits deliberately expose narrow data
//! operations. The terminal's OSC requests enter bus policy before a host
//! implementation sees them; no escape sequence directly opens a URL.
use std::time::Duration;
use web_time::Instant;

/// Shared program-reply credit and transport completion values.
pub mod reply;

/// Owner-local wake coalescing ends at publication, independently of host delivery.
mod wake;
pub use wake::PublicationWaker;

/// Ordered host work recorded by a synchronous transport without JS handles.
pub enum TransportEffect {
    /// Encoded user input; program replies use their charged Reply variant.
    Write(Vec<u8>),
    /// Program bytes carry their admitted receipt until host writer completion.
    Reply(reply::ReplyWrite),
    /// Grid and device-pixel dimensions for a host's asynchronous resize.
    Resize(PtySize),
    /// Release the connection after all preceding queued writes.
    Close,
}

/// Coalesced presentation requests handed to a platform after a core turn.
#[derive(Default)]
pub struct WakeRequests {
    /// Damage needs a refresh callback, never an immediate draw.
    pub frame: bool,
    /// Earliest pending blink, resize or synchronized-output timeout.
    pub deadline: Option<Instant>,
    /// Actual first/previous output times for hosts that coalesce until publication.
    pub arrival: Option<FrameArrival>,
}

/// Output timing travels by value, independently of event-loop delivery latency.
#[derive(Clone, Copy, Debug)]
pub struct FrameArrival {
    /// First request not yet represented by a snapshot.
    pub first: Instant,
    /// Last earlier output, including requests suppressed by the host wake latch.
    pub previous: Option<Instant>,
}

/// Cell and device-pixel dimensions passed to a PTY after resize.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PtySize {
    /// Number of terminal columns, clamped by the session to at least two.
    pub columns: u16,
    /// Number of terminal rows, clamped to at least one.
    pub rows: u16,
    /// Physical width reported to applications that query pixel size.
    pub pixel_width: u16,
    /// Physical height reported to applications that query pixel size.
    pub pixel_height: u16,
}

/// Byte-oriented connection to the terminal application.
pub trait Transport: Send {
    /// Writes bytes in order; callers never wait for a render frame.
    fn write(&mut self, bytes: &[u8]) -> Result<(), String>;
    /// True only when program writes need an explicit later completion.
    fn deferred_replies(&self) -> bool {
        false
    }
    /// Admits already charged program bytes to the same FIFO as user input.
    /// Deferred transports return Pending and later report the exact ticket.
    /// Ok never implies completion merely because a callback has been queued.
    fn write_reply(&mut self, reply: reply::ReplyWrite) -> Result<reply::ReplyDelivery, String> {
        self.write(&reply.bytes)?;
        Ok(reply::ReplyDelivery::Complete(reply.ticket))
    }
    /// Releases any host-retained immutable lease after owner receipt validation.
    /// Ordinary PTY writers have already dropped their lease before notifying.
    fn reply_completed(&mut self, _id: u64) {}
    /// Installs the owner's nonblocking completion route before parsing starts.
    /// Only asynchronous native writers need this; foreign hosts complete by ID.
    /// The callback carries a receipt only, never a reference to Session state.
    fn set_reply_completion(
        &mut self,
        _complete: Box<dyn Fn(reply::ReplyCompletion) + Send + Sync>,
    ) {
    }
    /// Notifies the application of a new terminal and pixel size.
    fn resize(&mut self, size: PtySize) -> Result<(), String>;
    /// Releases the host's connection when its session closes.
    fn close(&mut self) -> Result<(), String>;
    /// Drains deferred host work; native transports do their I/O synchronously.
    /// The web implementation owns a typed queue because JS objects are not
    /// Send. Moving bytes out after pump/feed avoids unsafe Send wrappers and
    /// lets a host callback run after the Session borrow has ended (§4.3).
    fn take_effect(&mut self) -> Option<TransportEffect> {
        None
    }
}

/// Display scheduling separate from terminal parsing and input I/O.
pub trait Waker: Send {
    /// Schedules at most one presentation for pending damage.
    fn request_frame(&mut self);
    /// Schedules a specific future blink or sync-timeout wakeup.
    fn wake_at(&mut self, instant: Instant);
    /// Releases a coalesced damage wake at the owner-side publication boundary.
    /// Single-threaded hosts may leave this empty; desktop output after this
    /// boundary must request another refresh even while the old frame is building.
    fn acknowledge_frame(&mut self) {}
    /// Moves deferred scheduling to the platform after synchronous core work.
    /// Direct event-loop implementations have already issued their requests;
    /// a buffered browser waker overrides this to return its coalesced state.
    fn take_requests(&mut self) -> WakeRequests {
        WakeRequests::default()
    }
}

/// Measures elapsed feed time without owning terminal state.
pub trait Clock: Send {
    /// Returns elapsed time since the supplied platform timestamp.
    fn elapsed_since(&self, start: Instant) -> Duration;
}

/// Real monotonic clock for browser and desktop hosts.
pub struct SystemClock;

impl Clock for SystemClock {
    /// Uses web-time's native or browser monotonic instant implementation.
    fn elapsed_since(&self, start: Instant) -> Duration {
        start.elapsed()
    }
}

/// Deferred clipboard work carries no browser object across the core seam.
pub enum ClipboardEffect {
    /// Policy-approved binary MIME operation; the host must answer its token.
    Mime(crate::clipboard::Transaction),
    /// Native paste events provide the text after a shortcut requests it.
    Read,
    /// OSC 52 text passes policy before a host writes it asynchronously.
    Write(String),
    /// A rich gesture writes both MIME representations in one clipboard item.
    WriteHtml {
        /// Escaped HTML for editors that accept styled content.
        html: String,
        /// Original text for plain editors and terminals.
        text: String,
    },
}

/// Text clipboard operations; a read result returns through a command.
pub trait Clipboard: Send {
    /// Starts MIME work; Ok means accepted, never completed. Hosts send a later Answer.
    fn mime(
        &mut self,
        _transaction: crate::clipboard::Transaction,
    ) -> Result<(), crate::clipboard::Status> {
        Err(crate::clipboard::Status::Unavailable)
    }
    /// Stores plain text after any terminal-origin policy interceptor.
    fn write_text(&mut self, text: &str) -> Result<(), String>;
    /// Stores rich HTML together with the required plain-text alternative.
    /// Hosts without rich support preserve useful paste behavior through text.
    fn write_html(&mut self, _html: &str, text: &str) -> Result<(), String> {
        self.write_text(text)
    }
    /// Reads plain text for a separate, policy-visible Paste command.
    fn request_text(&mut self) -> Result<Option<String>, String>;
    /// Platforms with asynchronous clipboards drain work after core policy.
    fn take_effect(&mut self) -> Option<ClipboardEffect> {
        None
    }
}

/// A deterministic clipboard until a native provider is approved (D-15).
#[derive(Default)]
pub struct MemoryClipboard {
    /// Last written value, intentionally scoped to one host session.
    pub text: Option<String>,
}

impl Clipboard for MemoryClipboard {
    /// Keeps the complete text so a later paste can return it unchanged.
    fn write_text(&mut self, text: &str) -> Result<(), String> {
        self.text = Some(text.to_owned());
        Ok(())
    }

    /// The host obtains the stored value and submits it separately.
    fn request_text(&mut self) -> Result<Option<String>, String> {
        Ok(self.text.clone())
    }
}

/// Host URL opening after scheme and gesture policy checks.
pub trait UrlOpener: Send {
    /// Opens one already-validated external URL.
    fn open(&mut self, url: &str) -> Result<(), String>;
    /// Deferred hosts transfer an approved URI after ending the Session borrow.
    fn take_effect(&mut self) -> Option<String> {
        None
    }
}

/// Host notification display after rate limiting and text sanitization.
pub trait Notifier: Send {
    /// Shows one bounded title and body pair.
    fn notify(&mut self, title: &str, body: &str) -> Result<(), String>;
    /// Starts structured host work without waiting for notification-center completion.
    /// Legacy hosts still receive useful text; unsupported tracking is explicit.
    fn notification(&mut self, effect: crate::notification::Effect) -> Result<(), String> {
        match effect {
            crate::notification::Effect::Show { notification, .. } => {
                self.notify(&notification.title, &notification.body)
            }
            _ => Err("notification lifecycle is unsupported".into()),
        }
    }
}

/// No-op service for tests or hosts without an OS integration yet.
pub struct NoopService;

impl UrlOpener for NoopService {
    /// An unsupported opener reports the missing host capability.
    fn open(&mut self, _url: &str) -> Result<(), String> {
        Err("URL opening is unavailable on this host".into())
    }
}

impl Notifier for NoopService {
    /// A host without notifications does not claim to have shown one.
    fn notify(&mut self, _title: &str, _body: &str) -> Result<(), String> {
        Err("notifications are unavailable on this host".into())
    }
}

/// Environment owned by a session, supplied by its platform shell.
pub struct Services {
    /// Transport can be a WebSocket callback or desktop PTY writer.
    pub transport: Box<dyn Transport>,
    /// Frame and timer scheduling belongs to the platform event loop.
    pub waker: Box<dyn Waker>,
    /// Feed-budget time is replaceable by a deterministic manual clock.
    pub clock: Box<dyn Clock>,
    /// Clipboard requests remain outside the terminal byte parser.
    pub clipboard: Box<dyn Clipboard>,
    /// URL policy is checked before reaching this host capability.
    pub urls: Box<dyn UrlOpener>,
    /// Notifications never execute inside a VT parser callback.
    pub notifier: Box<dyn Notifier>,
}

impl Services {
    /// Builds the minimum useful host, with optional services kept local.
    #[must_use]
    pub fn new(transport: Box<dyn Transport>, waker: Box<dyn Waker>) -> Self {
        Self {
            transport,
            waker,
            clock: Box::new(SystemClock),
            clipboard: Box::<MemoryClipboard>::default(),
            urls: Box::new(NoopService),
            notifier: Box::new(NoopService),
        }
    }
}
