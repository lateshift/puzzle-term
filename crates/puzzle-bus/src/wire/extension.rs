//! Type-erased plugin values with a checked public wire identity (design §5.4).
//!
//! A local plugin retains its concrete type inside `Any`. The wire form is
//! captured at registration so JS and recordings can observe the same name
//! and value. Downcasting is confined to generic hook wrappers; callers do
//! not inspect `Any` directly.
//!
//! The registry stores one factory per stable name. Decoding an unregistered
//! extension fails before a box is made, so a typo cannot silently bypass
//! plugin policy. The value tree is checked both when a Rust plugin builds
//! it and when external data is decoded. A typed interceptor may rewrite its
//! payload; `refresh` then updates the wire snapshot for later subscribers.
use crate::{
    value::{ValidationReport, Value},
    wire::ToWire,
};
use std::{
    any::{Any, TypeId},
    collections::BTreeMap,
};

/// A plugin type with a stable `ext:<plugin>/<name>` identity.
pub trait WireExt: Send + Sized + 'static {
    /// Namespace and message name; changing it breaks recordings.
    const TYPE_NAME: &'static str;
    /// Produces a plain-data payload for external observation.
    fn to_value(&self) -> Value;
    /// Validates a wire payload before constructing the plugin type.
    fn from_value(value: &Value) -> Result<Self, ValidationReport>;
}

/// One registered plugin message with both typed and wire views.
pub struct Extension {
    /// Public identity chosen by the plugin author.
    name: &'static str,
    /// Concrete type for safe downcast by a generic hook.
    type_id: TypeId,
    /// Owned typed payload, never exposed to untyped callers.
    value: Box<dyn Any + Send>,
    /// Plain value prepared once at the edge, not per observer.
    wire: Value,
    /// The original concrete decoder preserves typed delivery across owner channels.
    factory: Factory,
}

/// Debug output deliberately excludes potentially private plugin payloads.
impl std::fmt::Debug for Extension {
    /// Prints the stable identity used by bus diagnostics.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Extension")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Safe construction and typed inspection of an extension.
impl Extension {
    /// Captures both views so later observation does not repeat conversion.
    /// A malformed plugin name or oversized value cannot enter the bus.
    pub fn new<T: WireExt>(value: T) -> Result<Self, ValidationReport> {
        validate_name(T::TYPE_NAME)?;
        // Conversion happens once here, not once per registered observer.
        let wire = value.to_value();
        crate::value::validate_tree(&wire)?;
        Ok(Self {
            name: T::TYPE_NAME,
            type_id: TypeId::of::<T>(),
            value: Box::new(value),
            wire,
            factory: |wire| Extension::new(T::from_value(wire)?),
        })
    }

    /// Copies a validated plugin value through its original typed decoder.
    /// Only an actual owned delivery calls this; unobserved events remain free.
    pub fn try_clone(&self) -> Result<Self, ValidationReport> {
        (self.factory)(&self.wire)
    }

    /// Stable type string for logging and recordings.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        self.name
    }

    /// Concrete identity used by the extension hook table.
    #[must_use]
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    /// Reads the payload only when the registered type matches.
    /// A mismatch produces `None` instead of a panic or unchecked cast.
    #[must_use]
    pub fn downcast_ref<T: WireExt>(&self) -> Option<&T> {
        self.value.downcast_ref::<T>()
    }

    /// Allows an extension interceptor to rewrite its own typed value.
    /// The wrapper refreshes the plain-data copy after this mutable borrow.
    #[must_use]
    pub fn downcast_mut<T: WireExt>(&mut self) -> Option<&mut T> {
        self.value.downcast_mut::<T>()
    }

    /// Refreshes the wire view after a typed interceptor changed the payload.
    /// Invalid rewritten data stops that command before external delivery.
    pub fn refresh<T: WireExt>(&mut self) -> Result<(), ValidationReport> {
        let Some(value) = self.downcast_ref::<T>() else {
            return Err(super::reject("extension type mismatch"));
        };
        let wire = value.to_value();
        crate::value::validate_tree(&wire)?;
        self.wire = wire;
        Ok(())
    }
}

/// Serializes the stored public view without exposing `Any`.
impl ToWire for Extension {
    /// Gives the same name used at generic registration.
    fn type_name(&self) -> &str {
        self.name
    }
    /// Clones only for an actual wire subscriber or recorder.
    fn to_value(&self) -> Result<Value, ValidationReport> {
        Ok(self.wire.clone())
    }
}

/// Factory signature for a plugin type registered under one wire name.
type Factory = fn(&Value) -> Result<Extension, ValidationReport>;

/// Registry for decoding external extension commands and events.
#[derive(Default)]
pub struct ExtensionRegistry {
    /// Names are sorted so generated documentation is deterministic.
    factories: BTreeMap<&'static str, Factory>,
}

/// Registration and checked decoding of plugin wire values.
impl ExtensionRegistry {
    /// Registers a concrete extension under its stable name.
    /// Duplicate names are rejected even if their Rust types differ.
    pub fn register<T: WireExt>(&mut self) -> Result<(), ValidationReport> {
        validate_name(T::TYPE_NAME)?;
        if self.factories.contains_key(T::TYPE_NAME) {
            return Err(super::reject("duplicate extension type"));
        }
        self.factories
            .insert(T::TYPE_NAME, |value| Extension::new(T::from_value(value)?));
        Ok(())
    }

    /// Rejects unknown plugin types before they reach a hook.
    /// The factory is selected only after the bounded tree walk succeeds.
    pub fn decode(&self, name: &str, value: &Value) -> Result<Extension, ValidationReport> {
        crate::value::validate_tree(value)?;
        let factory = self
            .factories
            .get(name)
            .ok_or_else(|| super::reject("unregistered extension type"))?;
        factory(value)
    }
}

/// Enforces a namespace and simple camelCase-like identifier segments.
/// The `ext:` prefix keeps plugin names separate from built-in domains.
fn validate_name(name: &str) -> Result<(), ValidationReport> {
    let Some(rest) = name.strip_prefix("ext:") else {
        return Err(super::reject("extension name needs ext: prefix"));
    };
    let Some((plugin, kind)) = rest.split_once('/') else {
        return Err(super::reject("extension name needs plugin/name"));
    };
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    };
    if valid(plugin) && valid(kind) {
        Ok(())
    } else {
        Err(super::reject("invalid extension identifier"))
    }
}
