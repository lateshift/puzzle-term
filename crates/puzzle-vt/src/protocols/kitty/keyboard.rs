//! Origin: kitty; <https://sw.kovidgoyal.net/kitty/keyboard-protocol/>.
//! Kitty keyboard negotiation and enhanced escape encoding (§6.8).
//!
//! The stack is bounded at 16 entries (§16). A push beyond that limit
//! evicts the oldest entry, so hostile output cannot grow storage. The
//! protocol defines each screen's stack independently; a screen switch
//! selects the other top entry without copying or merging either stack.
//!
//! A negotiated flag set is not an ANSI/DEC mode bit: it belongs to the
//! keyboard protocol's own stack. Full-screen editors can push flags in
//! the alternate screen and pop them on exit, while the shell's primary
//! flags remain intact. The parser routes only prefixed CSI u requests
//! here; unprefixed CSI u remains the cursor restore control (§6.5).
use crate::terminal::ActiveScreen;
use vte::Params;

/// Per-screen kitty keyboard flags, with fixed-capacity mode stacks.
///
/// Empty stacks imply the legacy keyboard protocol, represented by zero.
#[derive(Clone, Debug, Default)]
pub struct KittyModes {
    /// The main shell's current flags and saved outer modes.
    primary: Vec<u8>,
    /// Alternate-screen applications cannot overwrite main-screen flags.
    alternate: Vec<u8>,
}

impl KittyModes {
    /// Returns the selected screen's flags, or legacy zero when empty.
    ///
    /// This is a single top-entry read on the input path.
    #[must_use]
    pub fn flags(&self, screen: ActiveScreen) -> u8 {
        self.stack(screen).last().copied().unwrap_or(0)
    }

    /// Applies CSI >, <, = or ? u and appends any query reply.
    ///
    /// The caller buffers replies until the feed slice ends, preserving
    /// their order relative to other terminal identity reports (§6.12).
    pub fn control(
        &mut self,
        screen: ActiveScreen,
        params: &Params,
        prefix: u8,
        reply: &mut Vec<u8>,
    ) {
        // Omitted flags push zero; omitted pop count removes one entry.
        // VTE supplies bounded numeric components, not raw untrusted text.
        let first = params
            .iter()
            .next()
            .and_then(|p| p.first())
            .copied()
            .unwrap_or(0);
        // For set, mode 1 replaces, 2 ORs and 3 removes requested bits.
        let second = params
            .iter()
            .nth(1)
            .and_then(|p| p.first())
            .copied()
            .unwrap_or(1);
        match prefix {
            // Kitty's reply includes the query prefix, unlike an event.
            b'?' => reply.extend_from_slice(format!("\x1b[?{}u", self.flags(screen)).as_bytes()),
            b'>' => {
                let stack = self.stack_mut(screen);
                // The protocol retains the newest sixteen pushes when an
                // application exceeds the §16 cap (kitty keyboard page).
                if stack.len() == 16 {
                    stack.remove(0);
                }
                // Only the five specified flags are currently advertised.
                stack.push((first as u8) & 31);
            }
            b'<' => {
                let stack = self.stack_mut(screen);
                // A zero count means one pop, matching kitty's default.
                // Popping through an empty stack leaves legacy zero flags.
                for _ in 0..usize::from(first.max(1)).min(16) {
                    stack.pop();
                }
            }
            b'=' => {
                let stack = self.stack_mut(screen);
                let value = (first as u8) & 31;
                // Set operates on the current top; an empty stack behaves
                // as an implicit zero mode until the first set arrives.
                let current = stack.last().copied().unwrap_or(0);
                // Kitty mode 1 sets, mode 2 adds and mode 3 removes bits.
                let flags = match second {
                    2 => current | value,
                    3 => current & !value,
                    _ => value,
                };
                if let Some(top) = stack.last_mut() {
                    *top = flags;
                } else {
                    stack.push(flags);
                }
            }
            _ => {}
        }
    }

    /// Selects one screen's saved flags without exposing its storage.
    ///
    /// Screen switches never copy or merge the two histories.
    fn stack(&self, screen: ActiveScreen) -> &[u8] {
        match screen {
            ActiveScreen::Primary => &self.primary,
            ActiveScreen::Alternate => &self.alternate,
        }
    }

    /// Selects the bounded mutable mode stack for negotiation.
    ///
    /// The caller enforces the 16-entry cap before any push.
    fn stack_mut(&mut self, screen: ActiveScreen) -> &mut Vec<u8> {
        match screen {
            ActiveScreen::Primary => &mut self.primary,
            ActiveScreen::Alternate => &mut self.alternate,
        }
    }
}
