//! Opener requests carry data only; final scheme approval belongs to Session.
//! Conversion between typed terminal messages and bounded plain data (§5.7).
//! Select carries gesture intent while SelectionChanged carries resolved cells.
//! Both use JS-exact position validation in the dedicated selection codec.
//! The event contains no duplicate text copy during pointer movement.
//! An external dispatch retains the normal Origin and interceptor policies.
//! Wire allocation occurs only at external observation or replay boundaries.
//! Missing retained rows are validated by Session after decoding.
//!
//! Runtime dispatch handles Rust enums. This module alone touches `Value`
//! for built-in messages, keeping normal bus callbacks strongly typed.
//!
//! M6 activates keys, paste, host requests and VT facts at their reserved
//! type names. A public name changes only when its concrete payload type
//! exists, so old recordings cannot dispatch an untyped placeholder.
//! Terminal-origin clipboard and notification requests use the command
//! lane; a policy hook can veto them before any host service acts.
//! Already-observed facts use the event lane and cannot be vetoed.
//! The map parsers in `terminal_wire` check numbers before narrowing them;
//! a malformed value must return a ValidationReport rather than panic (R8).
//! Extension conversion stays isolated to registered plugin names.
//! Appearance is deliberately a two-value payload on the command lane:
//! CSS or OS theme labels never leak into the typed session model. A host
//! can report `light` or `dark`, while settings independently choose an
//! effective pair member. This keeps a future change from `auto` to an
//! explicit mode from rewriting the last observed system appearance.
//! ThemeChanged is a fact with both the stable selector and resolved
//! appearance. A subscriber can update surrounding UI without guessing
//! whether a dark-only scheme was selected as the user's light member.
//! Decoder validation requires both map keys and rejects extra keys, so
//! a replay cannot silently smuggle unrecognized theme state into a fact.
//! Viewport requests are presentation changes, not application input. Their
//! finite row counts reach the Session without an encoder or transport write.
//! The scrolled fact uses device pixels because hosts need the actual clamped
//! state, including fractional movement on displays with different cell sizes.
//! NewOutputChanged is a Boolean edge rather than an output counter; the M9
//! pill can subscribe without inspecting every terminal byte or frame.
//! Internal shortcut actions are typed host plumbing, not an externally
//! dispatchable way to skip the public key/text/paste policy lanes.
//! Binary clipboard alternatives reuse the public write kind and its policy hooks.
//! Their conversion delegates to a bounded decoder, preserving legacy HTML shapes.
//! Internal read requests remain absent from the externally dispatchable catalogue.
use super::{Command, Event, catalogue};
use super::{key_wire, terminal_wire};
use crate::theme::Appearance;
use puzzle_bus::{
    kinds::{Direction, KindDef, public_by_name},
    value::{ValidationReport, Value, validate_tree},
    wire::{ExtensionRegistry, FromWire, ToWire, check_shape, reject},
};
use std::collections::BTreeMap;

/// Rejects reserved, internal, unknown and misshapen external messages.
fn checked_kind(
    catalogue: &'static [KindDef],
    direction: Direction,
    name: &str,
    value: &Value,
) -> Result<&'static KindDef, ValidationReport> {
    // A bounded walk precedes matching, so malformed nested data cannot
    // bypass limits by choosing a simple top-level message kind.
    validate_tree(value)?;
    // Lookup excludes reserved rows, even if their advertised outer shape
    // happens to match. Their real typed payload is not available yet.
    let kind = public_by_name(catalogue, direction, name)
        .ok_or_else(|| reject("unknown, reserved or internal bus kind"))?;
    // Check the simple outer shape before any variant-specific decoding;
    // this gives callers a stable payload location in validation reports.
    // Pre-G1 recordings used a unit command-start fact; decoding keeps that
    // missing label, while new observations advertise the map in the catalogue.
    if !(direction == Direction::Event && kind.variant == "CommandStarted" && value == &Value::Null)
    {
        check_shape(kind.shape, value)?;
    }
    Ok(kind)
}

/// Rejects misspelled host appearance strings before bus dispatch.
fn appearance(value: &str) -> Result<Appearance, ValidationReport> {
    match value {
        "light" => Ok(Appearance::Light),
        "dark" => Ok(Appearance::Dark),
        _ => Err(reject("appearance must be light or dark")),
    }
}

/// Returns the stable wire spelling of an effective appearance.
fn appearance_name(value: Appearance) -> &'static str {
    match value {
        Appearance::Light => "light",
        Appearance::Dark => "dark",
    }
}

/// Commands serialize only when an external subscriber requests a wire view.
impl ToWire for Command {
    /// Returns a stable type identity independent of Rust spelling.
    fn type_name(&self) -> &str {
        // Commands that originate from OSC still have stable public names;
        // their trusted `Origin::Terminal` is attached outside this value.
        // Wire identities remain stable if Rust variant names are refactored.
        match self {
            Self::Copy(_) => "input/copy",
            Self::Key(_) => "input/key",
            Self::Scroll(_) => "viewport/scroll",
            Self::Select(_) => "terminal/select",
            Self::Search(_) => "terminal/search",
            Self::ScrollTo(_) => "viewport/scrollTo",
            Self::Mouse(_) => "input/mouse",
            Self::Pointer(_) => "input/pointer",
            Self::Wheel { .. } => "input/wheel",
            Self::BindingAction { .. } => "internal/bindingAction",
            Self::Text(_) => "input/text",
            Self::Preedit(_) => "input/preedit",
            Self::Focus(_) => "input/focus",
            Self::Paste(_) => "input/paste",
            Self::ClipboardWrite(_) => "platform/clipboardWrite",
            Self::ClipboardRead => "internal/clipboardRead",
            Self::DesktopNotification(_) => "internal/desktopNotification",
            Self::Drop(_) => "internal/drop",
            Self::Notify { .. } => "platform/notify",
            Self::SetPointerShape(_) => "platform/setPointerShape",
            Self::GlyphRasterized => "internal/glyphRasterized",
            Self::OverlayAction(_) => "internal/overlayAction",
            Self::RingBell => "platform/ringBell",
            Self::OpenLink(_) => "platform/openLink",
            Self::ChildExited(_) => "internal/childExited",
            Self::SetSystemAppearance(_) => "settings/systemAppearance",
            Self::Ext(extension) => extension.type_name(),
        }
    }

    /// Converts each real Rust payload to the matching plain shape.
    fn to_value(&self) -> Result<Value, ValidationReport> {
        // Clipboard text, notification fields and input strings allocate
        // only when an external subscriber actually requests a snapshot.
        // Cloning is deferred until an actual external conversion request;
        // normal typed dispatch never copies the committed input text.
        let value = match self {
            Self::Copy(target) => super::copy_wire::to_value(*target),
            Self::Key(event) => key_wire::to_value(event),
            Self::Scroll(rows) => Value::Number(*rows),
            Self::Select(request) => super::selection_wire::to_value(*request),
            // Search uses typed operations; only observers allocate a query map.
            Self::Search(request) => super::search_wire::to_value(request),
            Self::ScrollTo(target) => super::viewport_wire::target_value(*target),
            Self::Mouse(event) => super::mouse_wire::to_value(event),
            Self::Pointer(input) => super::pointer_wire::to_value(*input),
            Self::Wheel { delta, pointer } => super::wheel_wire::to_value(*delta, pointer),
            Self::BindingAction { .. } => return Err(reject("shortcut actions are internal")),
            Self::Text(text) => Value::String(text.clone()),
            // Preedit carries a caret in UTF-8 bytes, unlike DOM's UTF-16.
            // Delegate to its validator so recording cannot publish an
            // offset that external replay would reject as a split scalar.
            Self::Preedit(state) => super::preedit_wire::to_value(state)?,
            Self::Focus(focused) => Value::Bool(*focused),
            Self::Paste(text) => Value::String(text.clone()),
            Self::SetPointerShape(text) => Value::String(text.clone()),
            Self::ClipboardWrite(super::ClipboardContent::Text(text)) => {
                Value::String(text.clone())
            }
            // Both alternatives travel as one value under the same policy kind.
            Self::ClipboardWrite(super::ClipboardContent::Html { html, text }) => {
                super::copy_wire::html_value(html, text)
            }
            Self::ClipboardWrite(super::ClipboardContent::Mime { token, items }) => {
                super::clipboard_wire::to_value(*token, items)
            }
            Self::ClipboardRead | Self::DesktopNotification(_) | Self::Drop(_) => {
                return Err(reject("host transaction is internal"));
            }
            Self::Notify { title, body } => terminal_wire::notification(title, body),
            // Callback navigation and raster completions are trusted internal handoffs.
            Self::GlyphRasterized | Self::OverlayAction(_) => {
                return Err(reject(
                    "internal completion or overlay action is not wire-visible",
                ));
            }
            Self::RingBell => Value::Null,
            // The wire carries data only; Session repeats scheme checks after interceptors.
            Self::OpenLink(uri) => Value::String(uri.clone()),
            Self::ChildExited(info) => terminal_wire::exit_info(info),
            Self::SetSystemAppearance(value) => Value::String(appearance_name(*value).into()),
            Self::Ext(extension) => return extension.to_value(),
        };
        // Rust callers can construct oversize text without decoding a wire
        // value first; reject it before passing data to JS or a recorder.
        validate_tree(&value)?;
        Ok(value)
    }
}

/// Public decoding constructs only fully typed, active commands.
impl FromWire for Command {
    /// Bad type strings and payloads return a report before any enqueue.
    fn from_value(
        type_name: &str,
        value: &Value,
        extensions: &ExtensionRegistry,
    ) -> Result<Self, ValidationReport> {
        // Registered plugin types own their validation and do not occupy a
        // fixed built-in catalogue row beyond the shared extension mask.
        if type_name.starts_with("ext:") {
            return Ok(Self::Ext(extensions.decode(type_name, value)?));
        }
        // A known public spelling alone is insufficient: only an active row
        // can construct a variant with its real Rust payload type.
        let kind = checked_kind(catalogue::COMMANDS, Direction::Command, type_name, value)?;
        // The outer shape gate ran first; helpers now validate inner keys
        // and ranges before constructing a typed request.
        match (kind.variant, value) {
            // Reject unsupported search fields before replacing an active query.
            ("Search", Value::Map(_)) => Ok(Self::Search(super::search_wire::from_value(value)?)),
            ("Copy", Value::Map(_)) => Ok(Self::Copy(super::copy_wire::from_value(value)?)),
            ("Key", Value::Map(_)) => Ok(Self::Key(key_wire::from_value(value)?)),
            ("Select", Value::Map(_)) => {
                Ok(Self::Select(super::selection_wire::from_value(value)?))
            }
            ("Wheel", Value::Map(_)) => super::wheel_wire::from_value(value),
            ("Pointer", _) => super::pointer_wire::from_value(value).map(Self::Pointer),
            ("Mouse", Value::Map(_)) => Ok(Self::Mouse(super::mouse_wire::from_value(value)?)),
            ("Scroll", Value::Number(_)) => Ok(Self::Scroll(super::viewport_wire::number(value)?)),
            ("ScrollTo", Value::Map(_)) => {
                Ok(Self::ScrollTo(super::viewport_wire::parse_target(value)?))
            }
            ("Text", Value::String(text)) => Ok(Self::Text(text.clone())),
            ("Preedit", Value::Map(_)) => {
                Ok(Self::Preedit(super::preedit_wire::from_value(value)?))
            }
            // Focus is a Boolean, not a truthy wire value: "false" and 0
            // must never become a synthetic focus-in report to the child.
            ("Focus", Value::Bool(focused)) => Ok(Self::Focus(*focused)),
            ("Paste", Value::String(text)) => Ok(Self::Paste(text.clone())),
            ("ClipboardWrite", Value::String(text)) => {
                Ok(Self::ClipboardWrite(text.clone().into()))
            }
            ("ClipboardWrite", _) => super::clipboard_wire::from_value(value),
            ("SetPointerShape", Value::String(shape)) => Ok(Self::SetPointerShape(shape.clone())),
            ("Notify", Value::Map(_)) => {
                let (title, body) = terminal_wire::parse_notification(value)?;
                Ok(Self::Notify { title, body })
            }
            ("RingBell", Value::Null) => Ok(Self::RingBell),
            ("OpenLink", Value::String(uri)) if uri.len() <= 2048 => {
                Ok(Self::OpenLink(uri.clone()))
            }
            ("SetSystemAppearance", Value::String(value)) => {
                Ok(Self::SetSystemAppearance(appearance(value)?))
            }
            // An active row without conversion is a programmer error, but
            // reject safely if the catalogue and code drift in production.
            _ => Err(reject("active command has no typed decoder")),
        }
    }
}

/// Event snapshots use the same stable wire vocabulary as commands.
impl ToWire for Event {
    /// Returns the public identity for a concrete fact.
    fn type_name(&self) -> &str {
        // OSC 133 phases each keep their own stable identity so a replay
        // can distinguish editable input from output after execution.
        // Observers use protocol names rather than Rust spellings, making
        // recordings stable across internal refactors.
        match self {
            Self::FrameRendered(_) => "render/frameRendered",
            Self::TitleChanged(_) => "terminal/titleChanged",
            Self::ViewportScrolled(_) => "viewport/scrolled",
            Self::SelectionChanged(_) => "terminal/selectionChanged",
            Self::NewOutputChanged(_) => "viewport/newOutputChanged",
            Self::SearchRequested => "terminal/searchRequested",
            Self::IconNameChanged(_) => "terminal/iconNameChanged",
            Self::CwdChanged(_) => "terminal/cwdChanged",
            Self::ContextsChanged(_) => "terminal/contextsChanged",
            Self::Bell => "terminal/bell",
            Self::PromptStarted => "shell/promptStarted",
            Self::InputStarted => "shell/inputStarted",
            Self::CommandStarted(_) => "shell/commandStarted",
            Self::CommandFinished(_) => "shell/commandFinished",
            Self::GridResized { .. } => "terminal/gridResized",
            Self::Progress { .. } => "terminal/progress",
            Self::SessionExited(_) => "session/exited",
            Self::SessionCloseRequested => "session/closeRequested",
            Self::ThemeChanged { .. } => "settings/themeChanged",
            Self::Ext(extension) => extension.type_name(),
        }
    }

    /// Converts a fact only when someone records or subscribes to it.
    fn to_value(&self) -> Result<Value, ValidationReport> {
        // A missing shell exit status remains null, not numeric zero.
        // Grid and progress maps are built only for subscribers.
        // Event text is copied only for a recorder or external subscriber;
        // ordinary observers borrow the strongly typed event directly.
        let value = match self {
            // Fixed phase/pipeline arrays preserve unit correspondence in recorded facts.
            Self::FrameRendered(stats) => super::stats_wire::encode(stats),
            Self::TitleChanged(title) => Value::String(title.clone()),
            Self::ViewportScrolled(pixels) => Value::Number(*pixels),
            Self::SelectionChanged(range) => super::selection_wire::range_value(*range),
            Self::NewOutputChanged(state) => Value::Bool(*state),
            // UI intent has no terminal contents or mutable query payload.
            Self::SearchRequested => Value::Null,
            Self::IconNameChanged(name) | Self::CwdChanged(name) => Value::String(name.clone()),
            // The active path is a lossless value snapshot, not a replay of OSC starts.
            Self::ContextsChanged(contexts) => super::context_wire::encode(contexts),
            Self::Bell => Value::Null,
            Self::PromptStarted | Self::InputStarted => Value::Null,
            // One host-facing map preserves missing labels explicitly. Parser
            // truncation and external wire validation share the same byte bound.
            Self::CommandStarted(command) => super::shell_wire::started(command.as_deref()),
            Self::CommandFinished(exit) => terminal_wire::finished(*exit),
            Self::GridResized { columns, rows } => terminal_wire::grid(*columns, *rows),
            Self::Progress { state, percent } => terminal_wire::progress(*state, *percent),
            Self::SessionExited(info) => terminal_wire::exit_info(info),
            Self::SessionCloseRequested => Value::Null,
            Self::ThemeChanged { name, appearance } => {
                let mut map = BTreeMap::new();
                map.insert("name".into(), Value::String(name.clone()));
                map.insert(
                    "appearance".into(),
                    Value::String(appearance_name(*appearance).into()),
                );
                Value::Map(map)
            }
            Self::Ext(extension) => return extension.to_value(),
        };
        // Domain-generated facts may still exceed the public wire bound;
        // the external representation must obey the same size policy.
        validate_tree(&value)?;
        Ok(value)
    }
}

/// Replay and external adapters can validate facts without changing them.
impl FromWire for Event {
    /// Rejects forged internal and not-yet-implemented facts with a report.
    fn from_value(
        type_name: &str,
        value: &Value,
        extensions: &ExtensionRegistry,
    ) -> Result<Self, ValidationReport> {
        // Plugin facts use the same checked registration path as requests,
        // while built-in facts remain a closed typed set.
        if type_name.starts_with("ext:") {
            return Ok(Self::Ext(extensions.decode(type_name, value)?));
        }
        // Internal or reserved facts cannot be forged through replay, even
        // though their names appear in the generated protocol reference.
        let kind = checked_kind(catalogue::EVENTS, Direction::Event, type_name, value)?;
        // Replay validates even event facts; a broken recording cannot
        // inject out-of-range progress or dimensions into an observer.
        match (kind.variant, value) {
            // Recorded statistics cross the same checked boundary as external events.
            // Reject malformed counters before observers or HUD formatting see them.
            ("FrameRendered", Value::Map(_)) => {
                Ok(Self::FrameRendered(super::stats_wire::decode(value)?))
            }
            ("TitleChanged", Value::String(title)) => Ok(Self::TitleChanged(title.clone())),
            ("IconNameChanged", Value::String(name)) => Ok(Self::IconNameChanged(name.clone())),
            ("CwdChanged", Value::String(uri)) => Ok(Self::CwdChanged(uri.clone())),
            ("ContextsChanged", Value::Map(_)) => {
                Ok(Self::ContextsChanged(super::context_wire::decode(value)?))
            }
            ("ViewportScrolled", Value::Number(_)) => {
                Ok(Self::ViewportScrolled(super::viewport_wire::number(value)?))
            }
            ("SelectionChanged", Value::Map(_)) => Ok(Self::SelectionChanged(
                super::selection_wire::parse_range(value)?,
            )),
            ("NewOutputChanged", Value::Bool(state)) => Ok(Self::NewOutputChanged(*state)),
            // Non-null payloads cannot masquerade as the unit UI request fact.
            ("SearchRequested", Value::Null) => Ok(Self::SearchRequested),
            ("Bell", Value::Null) => Ok(Self::Bell),
            ("PromptStarted", Value::Null) => Ok(Self::PromptStarted),
            ("InputStarted", Value::Null) => Ok(Self::InputStarted),
            ("CommandStarted", _) => super::shell_wire::event(value),
            ("CommandFinished", Value::Map(_)) => {
                Ok(Self::CommandFinished(terminal_wire::parse_finished(value)?))
            }
            ("GridResized", Value::Map(_)) => {
                let (columns, rows) = terminal_wire::parse_grid(value)?;
                Ok(Self::GridResized { columns, rows })
            }
            ("Progress", Value::Map(_)) => {
                let (state, percent) = terminal_wire::parse_progress(value)?;
                Ok(Self::Progress { state, percent })
            }
            ("SessionExited", Value::Map(_)) => {
                Ok(Self::SessionExited(terminal_wire::parse_exit(value)?))
            }
            ("SessionCloseRequested", Value::Null) => Ok(Self::SessionCloseRequested),
            ("ThemeChanged", Value::Map(map)) => theme_event(map),
            _ => Err(reject("active event has no typed decoder")),
        }
    }
}

/// Decodes the theme fact’s exact two-field shape independently of event routing.
fn theme_event(map: &BTreeMap<String, Value>) -> Result<Event, ValidationReport> {
    // The reported name is an identifier, not a request to register new theme data.
    let Some(Value::String(name)) = map.get("name") else {
        return Err(reject("theme name must be text"));
    };
    // Appearance uses the same closed vocabulary as settings/systemAppearance.
    let Some(Value::String(value)) = map.get("appearance") else {
        return Err(reject("theme appearance must be text"));
    };
    // Extra fields cannot smuggle configuration through this read-only fact.
    if map.len() != 2 || name.is_empty() {
        return Err(reject("theme fact has unexpected fields"));
    }
    Ok(Event::ThemeChanged {
        name: name.clone(),
        appearance: appearance(value)?,
    })
}
