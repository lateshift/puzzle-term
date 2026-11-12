//! Test-only message domain proving the bus needs no terminal types.
//!
//! These small enums have independent mask bits and wire identities. Their
//! only role is exercising generic queues, policies and extension routing.
use crate::{
    kinds::{Availability, Direction, KindDef, Shape, Visibility},
    message::Message,
    value::{ValidationReport, Value},
    wire::{Extension, ExtensionRegistry, FromWire, ToWire},
};

/// Fixture requests with typed text and an extension seam.
#[derive(Debug)]
pub(super) enum Command {
    /// Mutable text demonstrates in-place policy rewriting.
    Text(String),
    /// Unit request can recursively trigger a cascade.
    RingBell,
    /// Plugin request carries its one checked erased value.
    Ext(Extension),
}

/// Fixture facts that do not depend on a terminal crate.
#[derive(Debug)]
pub(super) enum Event {
    /// Unit event probes the zero-allocation listener mask.
    Bell,
    /// Plugin event probes concrete TypeId filtering.
    Ext(Extension),
}

/// Command mask positions are intentionally distinct from terminal positions.
const COMMANDS: &[KindDef] = &[
    KindDef {
        variant: "Text",
        name: "fixture/text",
        direction: Direction::Command,
        visibility: Visibility::Public,
        availability: Availability::Active,
        shape: Shape::String,
        description: "Fixture text",
    },
    KindDef {
        variant: "RingBell",
        name: "fixture/ringBell",
        direction: Direction::Command,
        visibility: Visibility::Public,
        availability: Availability::Active,
        shape: Shape::Null,
        description: "Fixture bell request",
    },
    KindDef {
        variant: "Ext",
        name: "ext:<plugin>/<name>",
        direction: Direction::Command,
        visibility: Visibility::Public,
        availability: Availability::Active,
        shape: Shape::Any,
        description: "Fixture plugin request",
    },
];

/// Event mask positions test generic observers separately.
const EVENTS: &[KindDef] = &[
    KindDef {
        variant: "Bell",
        name: "fixture/bell",
        direction: Direction::Event,
        visibility: Visibility::Public,
        availability: Availability::Active,
        shape: Shape::Null,
        description: "Fixture bell fact",
    },
    KindDef {
        variant: "Ext",
        name: "ext:<plugin>/<name>",
        direction: Direction::Event,
        visibility: Visibility::Public,
        availability: Availability::Active,
        shape: Shape::Any,
        description: "Fixture plugin fact",
    },
];

/// Generic bus operations for fixture commands.
impl Message for Command {
    /// The third mask position identifies plugin requests.
    const EXT_INDEX: usize = 2;
    /// The complete fixture command catalogue.
    fn catalogue() -> &'static [KindDef] {
        COMMANDS
    }
    /// Index used for precomputed policy masks.
    fn kind_index(&self) -> usize {
        match self {
            Self::Text(_) => 0,
            Self::RingBell => 1,
            Self::Ext(_) => 2,
        }
    }
    /// Borrows a plugin request when present.
    fn extension(&self) -> Option<&Extension> {
        if let Self::Ext(value) = self {
            Some(value)
        } else {
            None
        }
    }
    /// Allows typed plugin policies to rewrite a request.
    fn extension_mut(&mut self) -> Option<&mut Extension> {
        if let Self::Ext(value) = self {
            Some(value)
        } else {
            None
        }
    }
    /// Enqueues a validated plugin request.
    fn from_extension(value: Extension) -> Self {
        Self::Ext(value)
    }
}

/// Generic bus operations for fixture facts.
impl Message for Event {
    /// The second mask position identifies plugin facts.
    const EXT_INDEX: usize = 1;
    /// The complete fixture event catalogue.
    fn catalogue() -> &'static [KindDef] {
        EVENTS
    }
    /// Index used for precomputed observer masks.
    fn kind_index(&self) -> usize {
        match self {
            Self::Bell => 0,
            Self::Ext(_) => 1,
        }
    }
    /// Borrows a plugin fact when present.
    fn extension(&self) -> Option<&Extension> {
        if let Self::Ext(value) = self {
            Some(value)
        } else {
            None
        }
    }
    /// Satisfies the shared message seam; facts remain read-only to observers.
    fn extension_mut(&mut self) -> Option<&mut Extension> {
        if let Self::Ext(value) = self {
            Some(value)
        } else {
            None
        }
    }
    /// Enqueues a validated plugin fact.
    fn from_extension(value: Extension) -> Self {
        Self::Ext(value)
    }
}

/// Test-only wire view of fixture commands.
impl ToWire for Command {
    /// Reports the fixture identity or registered extension name.
    fn type_name(&self) -> &str {
        match self {
            Self::Text(_) => "fixture/text",
            Self::RingBell => "fixture/ringBell",
            Self::Ext(value) => value.type_name(),
        }
    }
    /// Converts only when a fixture test requests plain data.
    fn to_value(&self) -> Result<Value, ValidationReport> {
        match self {
            Self::Text(text) => Ok(Value::String(text.clone())),
            Self::RingBell => Ok(Value::Null),
            Self::Ext(value) => value.to_value(),
        }
    }
}

/// Test-only wire decoding for typed fixture commands.
impl FromWire for Command {
    /// Rejects unrecognized fixture requests with a report.
    fn from_value(
        name: &str,
        value: &Value,
        extensions: &ExtensionRegistry,
    ) -> Result<Self, ValidationReport> {
        match (name, value) {
            ("fixture/text", Value::String(text)) => Ok(Self::Text(text.clone())),
            ("fixture/ringBell", Value::Null) => Ok(Self::RingBell),
            (name, _) if name.starts_with("ext:") => Ok(Self::Ext(extensions.decode(name, value)?)),
            _ => Err(crate::wire::reject("invalid fixture command")),
        }
    }
}

/// Test-only wire view of fixture events.
impl ToWire for Event {
    /// Reports the fixture fact identity.
    fn type_name(&self) -> &str {
        match self {
            Self::Bell => "fixture/bell",
            Self::Ext(value) => value.type_name(),
        }
    }
    /// Converts a fixture fact only when tested.
    fn to_value(&self) -> Result<Value, ValidationReport> {
        match self {
            Self::Bell => Ok(Value::Null),
            Self::Ext(value) => value.to_value(),
        }
    }
}

/// Test-only wire decoding for fixture events.
impl FromWire for Event {
    /// Rejects unknown fact names without panicking.
    fn from_value(
        name: &str,
        value: &Value,
        extensions: &ExtensionRegistry,
    ) -> Result<Self, ValidationReport> {
        match (name, value) {
            ("fixture/bell", Value::Null) => Ok(Self::Bell),
            (name, _) if name.starts_with("ext:") => Ok(Self::Ext(extensions.decode(name, value)?)),
            _ => Err(crate::wire::reject("invalid fixture event")),
        }
    }
}
