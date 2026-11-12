//! Notification values shared by origin parsers and Session policy (M18.4).
//! No clock or OS handle enters VT. Kitty defines the wire grammar; these values
//! describe completed requests that Session may authorize and send to its host.

/// Per-field scalar bound, shared by OSC 9, 777 and 99 at the policy boundary.
pub const MAX_TEXT: usize = 1024;
/// Bounds incomplete messages and live host correlation independently.
pub const MAX_NOTIFICATIONS: usize = 64;

/// Focus/visibility condition attached to an application notification.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Occasion {
    /// Always eligible, subject to the shared security policy.
    #[default]
    Always,
    /// Eligible only while the creating surface lacks keyboard focus.
    Unfocused,
    /// Eligible only while unfocused and not visible to the user.
    Invisible,
}
/// Completed notification, independent of any native presentation technology.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Notification {
    /// Protocol identifier; None is unique and must not replace another anonymous item.
    pub id: Option<String>,
    /// Plain text heading, never interpreted as markup.
    pub title: String,
    /// Plain text details, independent of the title's chunk stream.
    pub body: String,
    /// Low, normal or critical (0, 1, 2).
    pub urgency: u8,
    /// Host focus state decides eligibility at posting time.
    pub occasion: Occasion,
    /// Activating may request focus of the originating surface.
    pub focus: bool,
    /// Activating may queue an OSC 99 reply to the originating application.
    pub report: bool,
    /// Closing may queue an OSC 99 close reply.
    pub close_report: bool,
}
impl Default for Notification {
    /// Kitty defaults to normal urgency and focus on activation, without reporting.
    fn default() -> Self {
        Self {
            id: None,
            title: String::new(),
            body: String::new(),
            urgency: 1,
            occasion: Occasion::Always,
            focus: true,
            report: false,
            close_report: false,
        }
    }
}
/// Complete origin request; close is not a new notification and consumes no rate slot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Request {
    /// A complete title/body transaction, possibly replacing the same protocol ID.
    Post(Notification),
    /// Remove only this Session's matching protocol ID.
    Close(String),
}
