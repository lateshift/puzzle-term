//! Retain a ready response while socket reads continue under sink backpressure.
//!
//! Reserving the only sink slot before waiting for either output or credit
//! deadlocks the other producer: an idle credit receiver can prevent the
//! child's first output from reaching the client. Instead, receive one ready
//! item into this owner, then select sink capacity alongside socket reads.
//! Cancelling reserve never loses bytes because this owner survives each turn.
use crate::{
    protocol::{INPUT_WINDOW, Server},
    pty::Output,
    writer::{self, Packet},
};
use tokio::sync::mpsc::Permit;

/// One retained packet, in addition to the sink queue and its active write.
#[derive(Default)]
pub(crate) struct Pending {
    /// None means producers may deliver their next item; Some awaits sink space.
    packet: Option<Packet>,
    /// Input allowance becomes usable when its grant enters the sink FIFO.
    grant: u32,
}

/// Producers never acquire a sink slot while waiting for their own next item.
impl Pending {
    /// Prevents either producer from overwriting the retained response.
    pub(crate) fn is_empty(&self) -> bool {
        self.packet.is_none()
    }

    /// Preserves PTY output order; EOF is observed after earlier packets enqueue.
    pub(crate) fn output(&mut self, output: Option<Output>) -> Result<bool, String> {
        match output {
            Some(Output::Bytes(bytes)) => {
                // Encode while the received chunk is still owned; no await
                // separates dequeuing from retention, so cancellation is harmless.
                self.packet = Some(writer::packet(Server::Output(&bytes))?);
                Ok(false)
            }
            Some(Output::Eof) => Ok(true),
            Some(Output::Failed(error)) => Err(error),
            None => Err("PTY output worker stopped".into()),
        }
    }

    /// Returns only bytes the OS has accepted, retaining the grant until sent.
    pub(crate) fn completed(
        &mut self,
        result: Option<Result<u32, String>>,
    ) -> Result<bool, String> {
        let Some(bytes) = result else {
            return Ok(false);
        };
        // Closing this channel disables only completion reads. Socket input
        // and output still run until EOF, child exit or a transport error.
        self.grant = bytes?;
        self.packet = Some(writer::packet(Server::InputCredit(self.grant))?);
        Ok(true)
    }

    /// Commits without awaiting: a selected reserve cannot lose a response.
    pub(crate) fn send(
        &mut self,
        permit: Permit<'_, Packet>,
        credit: &mut u32,
    ) -> Result<(), String> {
        // This bound includes queued and active stdin bytes. Restoring more
        // than the original allowance would silently disable memory limits.
        *credit = credit
            .checked_add(self.grant)
            .filter(|n| *n <= INPUT_WINDOW)
            .ok_or("invalid input completion credit")?;
        let packet = self.packet.take().ok_or("missing pending response")?;
        // An output packet after a grant must not replenish that grant again.
        self.grant = 0;
        permit.send(packet);
        Ok(())
    }
}
