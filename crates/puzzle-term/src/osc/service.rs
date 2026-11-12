//! Optional owner work resumes on declared parser turns, never in a host callback.
//! The empty registry performs no source read, allocation or service callback.
//! Immutable effects share prepaid bytes; a protocol bounds its own outstanding
//! requests and releases them by completion controls on this same Session owner.
use super::{OscContext, Registry};
use crate::{
    messages::{Command, Event},
    session::host_events::HostEvent,
};
use puzzle_bus::Bus;
use std::{collections::VecDeque, sync::Arc};

/// Cold optional host work with protocol-defined framing and a stable event name.
/// The registered owner validates genuine user authority, keeps source lifetime
/// local and bills this allocation while a host or callback retains it. This
/// generic envelope contains no source range that a host could synchronously read.
pub struct OscHostEffect {
    /// Trusted static extension name, never a name chosen by PTY metadata.
    pub kind: &'static str,
    /// Complete immutable payload; Vec capacity stays in the owner's byte ledger.
    pub data: Arc<Vec<u8>>,
}

impl Registry {
    /// Local work may advance without a producer or external completion wake.
    /// External waits are deliberately excluded to avoid idle polling feeds.
    pub(crate) fn owner_work_pending(&self) -> bool {
        self.handlers.iter().any(|r| r.handler.owner_work_pending())
    }

    /// Call once in the current caller-declared turn, after source reconciliation.
    /// Handler credits were replenished at begin_turn, never by this callback.
    pub(crate) fn continue_owner_work(
        &mut self,
        terminal: &mut puzzle_vt::Terminal,
        bus: &mut Bus<Command, Event>,
    ) {
        // The registry stores handler owners, never a shared live Session. Each
        // callback borrows exactly its own endpoint arena for this bounded turn.
        for registration in &mut self.handlers {
            if !registration.handler.owner_work_pending() {
                continue;
            }
            let mut source = OscContext {
                terminal,
                endpoints: &mut registration.endpoints,
                replies: None,
            };
            registration.handler.continue_owner_work(&mut source, bus);
        }
    }

    /// Host observation is opt-in. Moving one effect per owner avoids a hidden
    /// draining loop controlled by a plugin. Each collection moves at most one
    /// effect per registration; the handler bounds its outstanding requests.
    pub(crate) fn take_host_effects(&mut self, output: &mut VecDeque<HostEvent>) {
        for registration in &mut self.handlers {
            if let Some(effect) = registration.handler.take_host_effect() {
                output.push_back(HostEvent::OscEffect(effect));
            }
        }
    }
}
