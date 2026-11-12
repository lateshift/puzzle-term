//! Bounded graphics decode waits retain priority input and ordered child EOF.
//! Only the platform worker decodes pixels; the Session owns reply ordering.
use super::*;

/// Image work pauses application parsing, never the Session's control/publication turns.
impl Owner {
    /// A completed decoder resumes accepted suffixes before pool EOF can end the owner.
    /// This also covers ordinary parse-budget suffixes in finite harness sessions.
    pub(super) fn drain_pending(&mut self) -> bool {
        // Empty feeds are not needed in the normal image-free pool path.
        // Avoid an extra Session service turn when no retained bytes exist.
        if self.session.pending_bytes() == 0 {
            return false;
        }
        // Bytes were already accepted from the fixed pool. Draining them before
        // observing EOF preserves the protocol reply barrier in finite probes too.
        self.session.feed(&[], Instant::now());
        self.parsed
            .store(self.session.parsed_bytes_total(), Ordering::Relaxed);
        // Host notifications and prior input still leave while image work waits.
        self.deliver();
        true
    }

    /// Submits once or sleeps on controls while the App worker owns the result.
    pub(super) fn wait_image_decode(&mut self) -> Result<bool, String> {
        if !self.session.image_decode_pending() {
            return Ok(false);
        }
        // A full queue retains its request and sleeps until DecodeReady. Once
        // submitted, take() returns None and the outstanding job is not duplicated.
        if !self.decode_blocked
            && let Some(request) = self.session.take_image_decode_request()
        {
            // Enabling native graphics without App's decoder is a configuration
            // error, not permission to perform expensive work on this thread.
            let decoder = self
                .decoder
                .as_ref()
                .ok_or("graphics enabled without native decoder")?;
            // Full admission returns ownership intact. A readiness control is
            // guaranteed by the worker's registration/queue-release ordering.
            if let Some(request) = decoder.submit(request, self.decode_reply.clone())? {
                self.session.return_image_decode_request(request);
                self.decode_blocked = true;
            }
        }
        self.deliver();
        // The decoder posts directly to this inbox; pending output cannot cause a spin.
        // Keys and frame requests share this priority inbox with completions.
        // The unread application pool never queues ahead of those controls.
        match self.control.recv() {
            Ok(control) => self.apply_control(control),
            Err(_) => self.stopping = true,
        }
        Ok(true)
    }
}

/// Child status follows all accepted application bytes, including pending images.
impl Owner {
    /// Drain EOF follows child-status send, so lifecycle never blocks behind native reaping.
    pub(super) fn finish_exit(&mut self) -> Result<(), String> {
        let Some(exit) = self.exit.take() else {
            return Ok(());
        };
        // The reader sends status before dropping its final data producer.
        // Thus observed EOF is also the ordering barrier for this nonblocking read.
        let info = exit.try_recv().map_err(|error| error.to_string())?;
        // Session may still retain a budgeted suffix after the pool becomes empty.
        // Priority controls retain a turn between those existing bounded feed calls.
        loop {
            // A final chunk can be the child's last write. EOF does not permit
            // dropping that chunk's decode result or the query reply behind it.
            if self.wait_reply_credit() || self.wait_image_decode()? {
                if self.stopping {
                    return Ok(());
                }
                continue;
            }
            let outcome = self.session.feed(&[], Instant::now());
            self.parsed
                .store(self.session.parsed_bytes_total(), Ordering::Relaxed);
            if outcome.pending_bytes == 0 {
                break;
            }
            self.controls()?;
        }
        self.session.apply(
            puzzle_term::SessionMessage::Command(
                puzzle_term::messages::Command::ChildExited(info),
                puzzle_term::Origin::Platform,
            ),
            Instant::now(),
        )
    }
}
