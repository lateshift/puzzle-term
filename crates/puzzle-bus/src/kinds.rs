//! Catalogue metadata shared by any consumer of the generic bus (design §5.7).
//!
//! A domain crate owns its concrete enum and table. This leaf crate defines
//! only the vocabulary needed to describe a row: direction, visibility,
//! availability and outer wire shape. Reserving a name does not make its
//! message dispatchable before the domain's Rust payload type exists.

/// Which lane owns a stable protocol identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// A request that may be rewritten or vetoed.
    Command,
    /// A fact already emitted by its domain owner.
    Event,
}

/// Whether external callers may access a kind once it is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    /// The type string participates in the public wire API.
    Public,
    /// Platform plumbing must remain inside the host.
    Internal,
}

/// Whether a reserved protocol name has a concrete Rust variant yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    /// A typed variant and wire converter exist now.
    Active,
    /// The named milestone will introduce the payload type and converter.
    Reserved(&'static str),
}

/// Fundamental payload shape checked before domain-specific fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Registered extensions define their own validated plain-data shape.
    Any,
    /// No payload is accepted.
    Null,
    /// Unicode text is required.
    String,
    /// A Boolean is required.
    Bool,
    /// A finite number is required.
    Number,
    /// Named fields are required.
    Map,
    /// Opaque bytes are permitted only by an internal kind.
    Bytes,
}

/// One stable protocol name and the status of its Rust implementation.
#[derive(Clone, Copy, Debug)]
pub struct KindDef {
    /// Rust variant spelling; reserved rows name their future variant.
    pub variant: &'static str,
    /// Stable `domain/name` or extension pattern.
    pub name: &'static str,
    /// Command or event lane.
    pub direction: Direction,
    /// Wire access policy independent of implementation status.
    pub visibility: Visibility,
    /// An unavailable kind cannot be decoded or dispatched.
    pub availability: Availability,
    /// Top-level wire shape.
    pub shape: Shape,
    /// Human-facing meaning used in generated references.
    pub description: &'static str,
}

/// Looks up only active, public names for a given lane.
#[must_use]
pub fn public_by_name(
    catalogue: &'static [KindDef],
    direction: Direction,
    name: &str,
) -> Option<&'static KindDef> {
    // A reserved public name remains visible in the reference but cannot
    // cause a partially implemented command to enter the bus.
    catalogue.iter().find(|kind| {
        kind.name == name
            && kind.direction == direction
            && kind.visibility == Visibility::Public
            && kind.availability == Availability::Active
    })
}
