//! Binding entry grammar, separate from stateful input matching (§14.9).
//!
//! The first equals sign is the grammar boundary; action text may contain
//! any later delimiter. Leader length and source size are bounded before
//! allocating a shortcut table from untrusted config or page options.
//! Prefixes affect routing rather than trigger identity, so two layers
//! cannot retain conflicting shortcuts merely by changing performable.
//! The clear entry has no trigger: it resets the accumulated layer table.
use super::{Action, Platform, Source, Trigger, action_parse};

/// Consumption/routing policy; none participates in trigger identity.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Prefixes {
    /// Execute the action and still encode the original key.
    pub unconsumed: bool,
    /// Fall through unless the action's current feature can execute.
    pub performable: bool,
    /// Host applies the action to sessions sharing this settings store.
    pub all: bool,
}

/// One effective shortcut, possibly a sequence of physical/logical steps.
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    /// Chords are compared in order, not as an unordered set.
    pub steps: Vec<Trigger>,
    /// Typed, validated payload ready for execution.
    pub action: Action,
    /// Consumption is independent of the source layer's priority.
    pub prefixes: Prefixes,
    /// Highest contributing layer after canonical merge.
    pub source: Source,
}

/// Computes identity without conflating it with consumption policy.
impl Binding {
    /// Normal form shared by layer replacement and introspection.
    pub fn canonical(&self) -> String {
        self.steps
            .iter()
            .map(Trigger::canonical)
            .collect::<Vec<_>>()
            .join(">")
    }
    /// Warns about browser chrome chords; embedded hosts can disable it.
    pub fn reserved(&self, platform: Platform) -> bool {
        // Browsers own tab/window management outside fullscreen keyboard
        // lock. Preserve the binding for embedded hosts, but report it.
        self.steps.iter().any(|step| {
            step.modifiers == platform.primary()
                && ["w", "t", "n", "q", "KeyW", "KeyT", "KeyN", "KeyQ"].contains(&step.key.as_str())
        })
    }
}

/// A clear directive differs from a binding, because it has no trigger.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// Drops every lower and earlier binding, including platform defaults.
    Clear,
    /// Validated typed binding to add or replace by canonical identity.
    Binding(Binding),
}

/// Validates external strings before they become executable actions.
impl Entry {
    /// Reports malformed entries rather than manufacturing a key/action.
    pub fn parse(text: &str, platform: Platform, source: Source) -> Result<Self, String> {
        if text == "clear" {
            return Ok(Self::Clear);
        }
        if text.len() > 4096 {
            return Err("binding exceeds 4 KiB".into());
        }
        let (mut trigger, action) = text.split_once('=').ok_or("expected trigger=action")?;
        let mut prefixes = Prefixes::default();
        // Prefixes can be combined in any order, but a typo cannot silently
        // turn an intended unconsumed action into a consuming one.
        while let Some((prefix, tail)) = trigger.split_once(':') {
            match prefix {
                "unconsumed" => prefixes.unconsumed = true,
                "performable" => prefixes.performable = true,
                "all" => prefixes.all = true,
                _ => return Err(format!("unknown binding prefix: {prefix}")),
            }
            trigger = tail;
        }
        let steps = trigger
            .split('>')
            .map(|step| Trigger::parse(step, platform))
            .collect::<Result<Vec<_>, _>>()?;
        if steps.len() > 16 {
            return Err("sequence exceeds 16 steps".into());
        }
        Ok(Self::Binding(Binding {
            steps,
            action: action_parse::parse(action)?,
            prefixes,
            source,
        }))
    }
}
