//! Contracts for domain-owned message enums (design §5, corrected 2026-09-29).
//!
//! The bus can route a command or fact without knowing its terminal meaning.
//! A domain enum supplies its mask position, catalogue, and optional plugin
//! extension view. Wire conversion is a separate fallible boundary so the
//! normal queued path never builds a `Value` merely to dispatch a message.
use crate::{
    kinds::{KindDef, Visibility},
    wire::{Extension, FromWire, ToWire},
};

/// Trusted source attached by the host rather than external wire data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Physical user input.
    User,
    /// Browser or operating-system lifecycle callback.
    Platform,
    /// Terminal application request subject to policy.
    Terminal,
    /// Validated public action submitted by an external API.
    Api,
    /// Typed plugin acting within the host.
    Plugin,
}

/// Domain-owned enum operations needed by generic queued dispatch.
pub trait Message: FromWire + ToWire + Send + 'static {
    /// Mask position reserved for this domain's extension variant.
    const EXT_INDEX: usize;

    /// Full catalogue, including names reserved for future typed variants.
    fn catalogue() -> &'static [KindDef];

    /// Stable bit position for this concrete enum variant.
    fn kind_index(&self) -> usize;

    /// Borrow the erased plugin value only when this is an extension.
    fn extension(&self) -> Option<&Extension>;

    /// Mutably borrow a plugin value before its wire snapshot is refreshed.
    fn extension_mut(&mut self) -> Option<&mut Extension>;

    /// Re-enter the domain enum after validating a typed extension value.
    fn from_extension(extension: Extension) -> Self;

    /// Public visibility fails closed if a domain forgets a catalogue row.
    fn visibility(&self) -> Visibility {
        Self::catalogue()
            .get(self.kind_index())
            .map_or(Visibility::Internal, |kind| kind.visibility)
    }
}
