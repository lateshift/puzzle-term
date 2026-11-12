//! Validates action arguments before they enter the typed matcher.
//!
//! Configuration is untrusted. Numeric ranges and escape sequences report
//! errors per entry instead of turning malformed shortcuts into input.
//!
//! A colon separates the action name from its payload only once. This is
//! important for CSI strings and ordinary text containing further colons.
//! Aliases translate into the portable vocabulary before argument checking.
//! An unsupported alias action reports an error rather than a no-op;
//! otherwise users would think a split or tab binding had been installed.
//! Unit actions explicitly reject payloads because ignored suffixes hide
//! configuration mistakes. The catalogue's availability is independent of
//! parsing: search can be represented today without pretending M8 is built.
//! Numeric arguments use signed steps, allowing backward scrolling and zoom.
//! Fractional pages accept finite numbers only; infinity could otherwise
//! create an unbounded viewport target when scrolling arrives in M8.
//! Text escaping happens at configuration time, not when the key is pressed.
//! The escape decoder reads Unicode scalar values, not arbitrary raw bytes.
//! A hex escape can express terminal control characters such as ESC, while
//! Unicode escapes reject surrogates because Rust strings must remain UTF-8.
//! Exactly two hex digits belong to a byte escape. A following letter is
//! literal text, preserving common readline bindings such as ESC followed
//! by b or f. A trailing backslash is rejected rather than silently dropped.
//! The Unicode accumulator is bounded to six digits before multiplication,
//! so hostile input cannot overflow the integer or make decoding panic.
//! Strings have already passed the binding's 4 KiB limit in parser.rs;
//! expansion cannot create more output scalars than there are input bytes.
//! Copy format is a closed vocabulary: text, VT replay, or styled HTML.
//! Profile and extension names are retained as names, because their owners
//! resolve existence separately from the syntax of a portable binding.
//! Missing arguments are rejected before allocating an action payload.
//! An empty sendText is not useful as a shortcut; ignore expresses swallowing
//! explicitly and does not accidentally depend on an empty string write.
//! Screen reset, prompt clear and history clear are separate actions. Their
//! names must not alias each other because their retained state differs.
//! Window actions are parsed for portability but remain unavailable until
//! the desktop application milestone supplies their host implementation.
//! The matchers use the resulting enum, never repeat string parsing on keys.
//! Error strings identify the category that failed, and SettingsStore adds
//! the entry index so valid sibling bindings can still be installed.
//! Escapes are not decoded for csi/esc: their payload is already a sequence
//! suffix and its explicit introducer is attached by the execution path.
//! These routines do not write to a transport or mutate terminal modes;
//! validation cannot execute input while loading a configuration file.
use super::{Action, aliases};
use crate::settings::ThemeMode;

/// Parses the action half of a binding, including alternate-spelling aliases.
pub(super) fn parse(text: &str) -> Result<Action, String> {
    let translated = aliases::translate(text);
    let (name, arg) = translated
        .split_once(':')
        .map_or((translated.as_str(), None), |(n, a)| (n, Some(a)));
    // Unit actions share argument rejection; payload actions validate their
    // own categories so a typo cannot silently select a different command.
    if let Some(action) = unit(name) {
        no_argument(arg)?;
        return Ok(action);
    }
    payload(name, arg)
}

/// Resolves the argument-free vocabulary without interpreting input data.
fn unit(name: &str) -> Option<Action> {
    Some(match name {
        "paste" => Action::Paste,
        "pasteSelection" => Action::PasteSelection,
        "selectAll" => Action::SelectAll,
        "selectCommandOutput" => Action::SelectCommandOutput,
        "copyCommandOutput" => Action::CopyCommandOutput,
        "clearScreen" => Action::ClearScreen,
        "clearScrollback" => Action::ClearScrollback,
        "reset" => Action::Reset,
        "scrollTop" => Action::ScrollTop,
        "scrollBottom" => Action::ScrollBottom,
        "search" => Action::Search,
        "searchNext" => Action::SearchNext,
        "searchPrev" => Action::SearchPrev,
        "zoomReset" => Action::ZoomReset,
        "toggleAppearance" => Action::ToggleAppearance,
        "reloadSettings" => Action::ReloadSettings,
        "openSettings" => Action::OpenSettings,
        "showKeybindings" => Action::ShowKeybindings,
        "toggleScreenReaderMode" => Action::ToggleScreenReaderMode,
        "toggleFullscreen" => Action::ToggleFullscreen,
        "newWindow" => Action::NewWindow,
        "closeWindow" => Action::CloseWindow,
        "ignore" => Action::Ignore,
        "unbind" => Action::Unbind,
        _ => return None,
    })
}

/// Checks typed payloads after aliases have been resolved.
fn payload(name: &str, arg: Option<&str>) -> Result<Action, String> {
    Ok(match name {
        "copy" => Action::Copy(match arg.unwrap_or("text") {
            "text" | "vt" | "html" => arg.unwrap_or("text").into(),
            _ => return Err("copy format must be text, vt or html".into()),
        }),
        "sendText" => Action::SendText(escapes(required(arg)?)?),
        "csi" => Action::Csi(required(arg)?.into()),
        "esc" => Action::Esc(required(arg)?.into()),
        "scrollLines" => Action::ScrollLines(integer(required(arg)?)?),
        "scrollPages" => Action::ScrollPages(number(required(arg)?)?),
        "jumpToPrompt" => Action::JumpToPrompt(integer(required(arg)?)?),
        "zoom" => Action::Zoom(integer(required(arg)?)?),
        "setAppearance" => Action::SetAppearance(match required(arg)? {
            "light" => ThemeMode::Light,
            "dark" => ThemeMode::Dark,
            "auto" => ThemeMode::Auto,
            _ => return Err("expected light, dark or auto".into()),
        }),
        "profile" => Action::Profile(required(arg)?.into()),
        "ext" => Action::Ext(required(arg)?.into()),
        _ => return Err(format!("unsupported action: {name}")),
    })
}

/// Signed magnitudes reject overflow instead of wrapping a navigation step.
fn integer(arg: &str) -> Result<i32, String> {
    arg.parse().map_err(|_| "expected signed integer".into())
}

/// A missing payload cannot silently become an empty extension name.
fn required(arg: Option<&str>) -> Result<&str, String> {
    arg.filter(|value| !value.is_empty())
        .ok_or_else(|| "action requires an argument".into())
}

/// Unit actions reject typos rather than discarding an unexpected argument.
fn no_argument(arg: Option<&str>) -> Result<(), String> {
    if arg.is_some() {
        Err("action takes no argument".into())
    } else {
        Ok(())
    }
}

/// Fractional pages must be finite so a future viewport stays bounded.
fn number(arg: &str) -> Result<f32, String> {
    let value: f32 = arg.parse().map_err(|_| "expected finite number")?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err("expected finite number".into())
    }
}

/// Decodes only the documented sendText escapes, with checked UTF-8 scalars.
pub(super) fn escapes(text: &str) -> Result<String, String> {
    let mut output = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        let escaped = match chars.next() {
            Some('n') => '\n',
            Some('t') => '\t',
            Some('r') => '\r',
            Some('\\') => '\\',
            Some('x') => {
                // Exactly two hex digits prevent a following letter being
                // swallowed as part of a control byte (portable text syntax).
                let a = chars
                    .next()
                    .and_then(|c| c.to_digit(16))
                    .ok_or("bad hex escape")?;
                let b = chars
                    .next()
                    .and_then(|c| c.to_digit(16))
                    .ok_or("bad hex escape")?;
                char::from_u32(a * 16 + b).ok_or("invalid hex scalar")?
            }
            Some('u') if chars.next() == Some('{') => unicode_escape(&mut chars)?,
            _ => return Err("invalid or incomplete text escape".into()),
        };
        output.push(escaped);
    }
    Ok(output)
}

/// Accepts one Unicode scalar; surrogates and overlong values are rejected.
fn unicode_escape(chars: &mut impl Iterator<Item = char>) -> Result<char, String> {
    let mut code = 0_u32;
    let mut digits = 0;
    for ch in chars.by_ref() {
        if ch == '}' && digits > 0 {
            return char::from_u32(code).ok_or_else(|| "invalid Unicode scalar".into());
        }
        digits += 1;
        if digits > 6 {
            return Err("Unicode escape is too long".into());
        }
        code = code * 16 + ch.to_digit(16).ok_or("invalid Unicode hex digit")?;
    }
    Err("unterminated Unicode escape".into())
}
