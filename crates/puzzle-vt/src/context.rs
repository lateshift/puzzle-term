//! Bounded, auxiliary context metadata, independent of rendering and host actions.
//! UAPI.15 https://uapi-group.org/specifications/specs/osc_context/ defines the
//! active path: a repeated start replaces metadata and terminates descendants.
//! This information is untrusted descriptive text, never proof of privilege.
//! No cell, row or frame contains context data; unused sessions allocate nothing.

/// Decoded-value checks shared by parsing and replay.
mod validation;

/// Maximum retained active depth; overflow preserves the earlier safety context.
pub const MAX_DEPTH: usize = 64;
/// Decoded context IDs contain at most 64 printable ASCII bytes (UAPI.15).
pub const MAX_ID: usize = 64;
/// Metadata strings use the protocol's 255-character ABNF bound.
pub const MAX_TEXT: usize = 255;

/// Recognized start-field names; arbitrary keys never enter retained state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldKey {
    /// Context category, such as remote, elevate or container.
    Type,
    /// User running the reporting process.
    User,
    /// Host running the reporting process.
    Hostname,
    /// Machine identifier as hexadecimal text with optional hyphens.
    MachineId,
    /// Boot identifier as hexadecimal text with optional hyphens.
    BootId,
    /// Decimal process ID; text preserves all 64 bits for JSON hosts.
    Pid,
    /// Decimal pidfd inode identity, also preserving all 64 bits.
    PidFdId,
    /// Reporting process name.
    Comm,
    /// Working directory reported by a shell or command.
    Cwd,
    /// Command line, including an explicitly empty value.
    Cmdline,
    /// Name of a virtual machine.
    Vm,
    /// Name of a container.
    Container,
    /// User of the new privilege or remote context.
    TargetUser,
    /// Host of the remote context.
    TargetHost,
    /// Identifier of the login session.
    SessionId,
}

impl FieldKey {
    /// The wire and host spelling comes from the defining protocol, not a UI label.
    pub fn name(self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::User => "user",
            Self::Hostname => "hostname",
            Self::MachineId => "machineid",
            Self::BootId => "bootid",
            Self::Pid => "pid",
            Self::PidFdId => "pidfdid",
            Self::Comm => "comm",
            Self::Cwd => "cwd",
            Self::Cmdline => "cmdline",
            Self::Vm => "vm",
            Self::Container => "container",
            Self::TargetUser => "targetuser",
            Self::TargetHost => "targethost",
            Self::SessionId => "sessionid",
        }
    }
    /// Unknown names are ignored independently of neighboring valid fields.
    pub fn parse(name: &str) -> Option<Self> {
        // The closed table also bounds the number of distinct retained fields.
        Some(match name {
            "type" => Self::Type,
            "user" => Self::User,
            "hostname" => Self::Hostname,
            "machineid" => Self::MachineId,
            "bootid" => Self::BootId,
            "pid" => Self::Pid,
            "pidfdid" => Self::PidFdId,
            "comm" => Self::Comm,
            "cwd" => Self::Cwd,
            "cmdline" => Self::Cmdline,
            "vm" => Self::Vm,
            "container" => Self::Container,
            "targetuser" => Self::TargetUser,
            "targethost" => Self::TargetHost,
            "sessionid" => Self::SessionId,
            _ => return None,
        })
    }
}

/// One validated start field; numeric identities remain lossless decimal text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Field {
    /// Recognized metadata role.
    pub key: FieldKey,
    /// Decoded UTF-8 value; controls, malformed escapes and oversized values are rejected.
    pub value: String,
}

/// One context in the active root-to-leaf path (the final element is active).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Context {
    /// Application-chosen identifier, unique only within this session.
    pub id: String,
    /// Known metadata only, at most one value per recognized key.
    pub fields: Vec<Field>,
}

impl Context {
    /// Borrows a field without constructing a string or copying the context stack.
    pub fn get(&self, key: FieldKey) -> Option<&str> {
        // At most fifteen fields are visited, independently of grid/history size.
        self.fields
            .iter()
            .find(|f| f.key == key)
            .map(|f| f.value.as_str())
    }
}

impl Field {
    /// Validate a decoded value for typed construction and external event replay.
    pub fn new(key: FieldKey, value: String) -> Option<Self> {
        validation::valid(key, &value).then_some(Self { key, value })
    }
}
