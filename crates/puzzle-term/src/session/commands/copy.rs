//! Clipboard actions stay interceptable at both request and write boundaries.
//!
//! Copy and copy-on-select submit a typed Copy request. Its handler extracts
//! the approved source and queues ClipboardWrite with the original trusted
//! origin; the clipboard policy can then rewrite or veto the actual contents.
//!
//! Extracting HTML and its plain alternative uses the same cached range and live
//! trim preference. No asynchronous read can change the range between formats:
//! core dispatch is synchronous. Both payloads then share the existing clipboard
//! write kind, so a single policy veto prevents every MIME side effect.

use super::Handler;
use crate::{
    keybind::Action,
    messages::{Command, Event},
    selection::CopyTarget,
    vt::view::Format,
};
use puzzle_bus::{Origin, hooks::HookCtx};

/// Handler borrows selection and services without platform APIs in the core.
impl Handler<'_> {
    /// Produces the requested serializer representation only for an available source.
    pub(super) fn copy(&mut self, target: CopyTarget, ctx: &mut HookCtx<'_, Command, Event>) {
        // This explicit operation can allocate; no text is copied on frame assembly.
        if let Some(text) = self.selection.copy(self.terminal, target) {
            let command = if target == CopyTarget::Selection(Format::Html) {
                // Derive both alternatives from the same accepted tracked interval.
                let plain = self
                    .selection
                    .copy(self.terminal, CopyTarget::Selection(Format::PlainText))
                    .unwrap_or_default();
                Command::ClipboardWrite(crate::messages::ClipboardContent::Html {
                    html: text,
                    text: plain,
                })
            } else {
                // Ordinary text and VT remain the existing single-text transaction.
                Command::ClipboardWrite(text.into())
            };
            ctx.submit(command, ctx.origin().unwrap_or(Origin::User));
        }
    }
    /// Maps the validated shortcut format to VT’s closed serializer vocabulary.
    pub(super) fn copy_action(
        &mut self,
        action: &Action,
        ctx: &mut HookCtx<'_, Command, Event>,
    ) -> bool {
        // Only clipboard actions enter this branch; others continue to their owner.
        let target = match action {
            Action::Copy(format) => CopyTarget::Selection(match format.as_str() {
                "vt" => Format::Vt,
                "html" => Format::Html,
                _ => Format::PlainText,
            }),
            Action::CopyCommandOutput => CopyTarget::CommandOutput,
            _ => return false,
        };
        // The request itself can be intercepted before extraction, just like Select.
        ctx.submit(Command::Copy(target), ctx.origin().unwrap_or(Origin::User));
        true
    }
}
