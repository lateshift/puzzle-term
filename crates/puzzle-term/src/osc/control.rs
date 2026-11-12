//! Host provisioning/revocation is an owner message, never a parsed OSC grant.
//! The selected registration lends only its own source namespace to the control.
//! No key is stored in a generic core field, and no synchronous host callback
//! or shared handler reference escapes. Only the optional handler interprets
//! its typed extension. Public wire dispatch cannot construct this message.
use super::{OscContext, Registry};
use crate::messages::{Command, Event};
use puzzle_bus::{Bus, wire::Extension};

/// Cold trusted operations preserve the existing one-owner protocol registry.
impl Registry {
    /// Refuses missing registrations before the payload can reach a sibling.
    /// Source epochs were synchronized by the Session before this call. A
    /// control cannot spend authentication work or reorder the pending request.
    pub(crate) fn control(
        &mut self,
        number: u16,
        control: &mut Extension,
        terminal: &mut puzzle_vt::Terminal,
        bus: &mut Bus<Command, Event>,
    ) -> Result<(), String> {
        let index = self
            .numbers
            .iter()
            .position(|n| *n == number)
            .ok_or("OSC handler is not registered")?;
        let registration = &mut self.handlers[index];
        // Host controls may capture/release source owners, but cannot issue
        // a reply ahead of the retained parser boundary through this context.
        let mut source = OscContext {
            terminal,
            endpoints: &mut registration.endpoints,
            replies: None,
        };
        registration.handler.control(control, &mut source, bus)
    }
}
