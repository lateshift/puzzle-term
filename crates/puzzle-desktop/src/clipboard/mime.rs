//! Native MIME clipboard transactions run on the harness UI owner, never Session.
//! arboard supports text, HTML with a plain alternative, and a single RGBA image.
//! Unsupported combinations fail before touching the clipboard; no partial write
//! is acknowledged as a successful multi-representation transaction.
//! The event-loop owner performs native clipboard I/O after receiving an effect.
//! The Session thread has already applied read/write policy and continues parsing.
//! Completion is posted back as an Answer, preserving the owner's reply ordering.
//! Each transaction has one result, including unsupported formats and OS errors.
//! The host never acknowledges merely enqueueing work or decoding an image.
//! Text plus HTML can be installed as one arboard operation with alternatives.
//! An image can be installed as one decoded RGBA operation.
//! Other mixed representations are rejected before modifying the clipboard;
//! installing only a subset would falsely acknowledge an atomic protocol write.
//! An empty MIME write clears the clipboard, distinct from an empty text value.
//! Reads request only alternatives named by policy; absent types are skipped.
//! A name-list query is itself policy-controlled by Session's default denial rule.
//! Paste gestures list native types without decoding every available image.
//! macOS changeCount identifies the exact observed clipboard generation.
//! It changes even if a user restores earlier bytes, preventing stale capability use.
//! A granted read checks the generation on both sides of OS I/O.
//! If clipboard ownership changes during the read, all returned data is denied.
//! Unrestricted reads enabled by settings do not need a gesture generation.
//! Hosts lacking revision tracking retain their existing ordinary text paste path.
//! Native paste observations contain no password: embed generates one securely.
//! The password remains in Session protocol state rather than this OS adapter.
//! PNG conversion is separately bounded before expansion and after encoding.
//! No system clipboard is touched by the pure PNG unit fixtures.
//! Actual clipboard UI behavior stays a human review item in both native hosts.
//! A missing native representation is not equivalent to empty clipboard contents.
//! The adapter returns ENOSYS when none of the approved types can be retrieved.
use puzzle_term::clipboard::{Answer, MimeData, Operation, Status, Transaction};
/// Bounded PNG conversion is independent of the user's actual system clipboard.
mod png;

/// Finishes an OS operation and produces the message the Session can acknowledge.
pub(crate) fn complete(transaction: Transaction) -> Answer {
    // Observe before OS reads so a grant cannot disclose newer clipboard contents.
    let revision = revision();
    let permitted =
        transaction.expected_revision.is_none() || transaction.expected_revision == revision;
    // Denial happens before even opening the system clipboard object.
    let result = if !permitted {
        Err(Status::Denied)
    } else {
        arboard::Clipboard::new().map_err(|_| Status::Io).and_then(
            |mut clipboard| match transaction.request.operation {
                Operation::Write(items) => write(&mut clipboard, &items).map(|()| Vec::new()),
                Operation::Read(names) => read(&mut clipboard, &names),
            },
        )
    };
    // Check again after I/O to reject an ownership change while data was copied.
    let after = self::revision();
    let result = if transaction.expected_revision.is_some() && after != revision {
        Err(Status::Denied)
    } else {
        result
    };
    // Even a native failure completes the token; silent success would strand a client.
    match result {
        Ok(items) => Answer {
            token: transaction.token,
            revision: after,
            status: Status::Done,
            items,
        },
        Err(status) => Answer {
            token: transaction.token,
            revision: after,
            status,
            items: Vec::new(),
        },
    }
}
/// Validates every requested representation before performing one atomic OS update.
fn write(clipboard: &mut arboard::Clipboard, items: &[MimeData]) -> Result<(), Status> {
    // Missing data must not masquerade as a successful empty representation.
    if items.is_empty() {
        // An empty completed write is the protocol's clipboard clear operation.
        return clipboard.clear().map_err(|_| Status::Io);
    }
    // Locate alternatives without changing the original clipboard as validation runs.
    let text = items.iter().find(|v| v.mime == "text/plain");
    let html = items.iter().find(|v| v.mime == "text/html");
    // arboard can atomically install HTML with its optional plain-text fallback.
    if items.len() <= 2
        && items
            .iter()
            .all(|v| matches!(v.mime.as_str(), "text/plain" | "text/html"))
    {
        // Text formats require valid UTF-8; PNG and other binary formats do not.
        let text = text
            .map(|v| std::str::from_utf8(&v.data).map_err(|_| Status::Invalid))
            .transpose()?;
        if let Some(html) = html {
            let html = std::str::from_utf8(&html.data).map_err(|_| Status::Invalid)?;
            return clipboard.set_html(html, text).map_err(|_| Status::Io);
        }
        if let Some(text) = text {
            return clipboard.set_text(text).map_err(|_| Status::Io);
        }
    }
    // The image operation cannot preserve unrelated text alternatives atomically.
    if let [item] = items
        && item.mime == "image/png"
    {
        // Decode and validate expansion limits before handing pixels to the OS.
        let image = png::decode(&item.data)?;
        return clipboard.set_image(image).map_err(|_| Status::Io);
    }
    // arboard has no arbitrary/mixed image+text transaction. Never drop alternatives.
    Err(Status::Unavailable)
}
/// Reads only explicitly policy-approved types, preserving binary image content.
fn read(clipboard: &mut arboard::Clipboard, names: &[String]) -> Result<Vec<MimeData>, Status> {
    // Collect only requested representations; a failed read returns no partial payload.
    let mut items = Vec::new();
    // Bound the combined result, not just each individually valid alternative.
    let mut total = 0usize;
    // Keep the request order so the Session can emit contiguous MIME reply chunks.
    for name in names {
        let bytes = match name.as_str() {
            "text/plain" => match clipboard.get_text() {
                Ok(text) => text.into_bytes(),
                Err(arboard::Error::ContentNotAvailable) => continue,
                Err(error) => return Err(map_error(error)),
            },
            "image/png" => match clipboard.get_image() {
                Ok(image) => png::encode(image)?,
                Err(arboard::Error::ContentNotAvailable) => continue,
                Err(error) => return Err(map_error(error)),
            },
            // Listing reports only formats this host can actually return.
            "." => available(clipboard).join(" ").into_bytes(),
            _ => continue,
        };
        // Native clipboard data is untrusted independently of the OSC request size.
        total = total.saturating_add(bytes.len());
        if total > puzzle_term::clipboard::MAX_BYTES {
            return Err(Status::TooLarge);
        }
        items.push(MimeData {
            mime: name.clone(),
            data: bytes.into(),
        });
    }
    if items.is_empty() {
        Err(Status::Unavailable)
    } else {
        Ok(items)
    }
}
/// Absence is protocol unsupported; permission/OS failures must never look like empty data.
fn map_error(error: arboard::Error) -> Status {
    match error {
        arboard::Error::ContentNotAvailable => Status::Unavailable,
        _ => Status::Io,
    }
}

/// Clipboard generation is queried only during native gestures or requested reads.
#[cfg(target_os = "macos")]
pub(crate) fn revision() -> Option<u64> {
    u64::try_from(objc2_app_kit::NSPasteboard::generalPasteboard().changeCount()).ok()
}

/// Listing native types does not decode a potentially large clipboard image.
#[cfg(target_os = "macos")]
fn available(_clipboard: &mut arboard::Clipboard) -> Vec<String> {
    // Native type identifiers describe availability without fetching clipboard bytes.
    let mut text = false;
    let mut image = false;
    if let Some(types) = objc2_app_kit::NSPasteboard::generalPasteboard().types() {
        // Multiple OS image aliases advertise one supported protocol representation.
        for kind in types.iter() {
            // Standard public UTIs avoid unsafe access to Objective-C extern constants.
            match kind.to_string().as_str() {
                "public.utf8-plain-text" => text = true,
                "public.png" | "public.tiff" => image = true,
                _ => {}
            }
        }
    }
    let mut names = Vec::new();
    if text {
        names.push("text/plain".into());
    }
    if image {
        names.push("image/png".into());
    }
    names
}

/// Other native clipboard backends expose representations only through their reads.
#[cfg(not(target_os = "macos"))]
fn available(clipboard: &mut arboard::Clipboard) -> Vec<String> {
    let mut names = Vec::new();
    if clipboard.get_text().is_ok() {
        names.push("text/plain".into());
    }
    if clipboard.get_image().is_ok() {
        names.push("image/png".into());
    }
    names
}
/// Hosts without a trustworthy revision cannot issue a default-policy grant.
#[cfg(not(target_os = "macos"))]
pub(crate) fn revision() -> Option<u64> {
    None
}

/// A gesture lists supported native alternatives and retains its ordinary text fallback.
pub(crate) fn paste(surface: &puzzle_embed::Surface) -> Result<(), String> {
    // This function is invoked by an actual host paste gesture, never terminal output.
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    // The listing and text fallback must come from one unchanged clipboard revision.
    let before = revision();
    let text = clipboard.get_text().ok();
    // Inspect type metadata even when no plain-text representation exists.
    let mimes = available(&mut clipboard);
    // Session decides mode precedence; the host does not maintain a second mode flag.
    if let Some(revision) = before.filter(|before| Some(*before) == self::revision()) {
        surface
            .clipboard_paste(false, revision, mimes, text)
            .map_err(|error| error.to_string())
    } else if let Some(text) = text {
        // An untrackable clipboard retains the existing text-only paste behavior.
        surface.paste(text).map_err(|error| error.to_string())
    } else {
        Ok(())
    }
}
