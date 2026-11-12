//! Kitty actions reference: <https://sw.kovidgoyal.net/kitty/graphics-protocol/>.
//! Ordered kitty actions and atomic worker-result commit (M15.2).
//! A transmission never replaces pixels or placements until every byte validates.
use super::{base64, control::Control};
use crate::images::{
    DecodeRequest, DecodeResult, Format, ImageError, Store,
    store::{Image, Transfer},
};
use crate::vt::Terminal;

/// The Session owns command interpretation; decoding is a platform message round trip.
impl Store {
    /// Processes one bounded graphics body, appending any reply to VT's ordered lane.
    pub fn command(&mut self, body: &[u8], terminal: &mut Terminal) {
        // ED/RIS may precede this APC in the same read. Retire their anchors now,
        // before a put or a replacement can resolve an image against stale state.
        self.reconcile(terminal);
        // Separate syntax from feature admission so unsupported commands can
        // still echo their valid image and placement identifiers in an error.
        let (control, payload) = match Control::syntax(body) {
            Ok(request) => request,
            Err(error) => {
                // Syntax errors cannot be dispatched, but a recoverable numeric ID
                // still gives the sender a useful acknowledgement rather than silence.
                reply(
                    &Control::reply_headers(body),
                    Err(error),
                    &mut terminal.replies,
                );
                // A broken chunk aborts this transfer as a whole. Its prefix must
                // never become the first bytes of an unrelated later image.
                self.transfer = None;
                return;
            }
        };
        // Continuations have no image ID of their own. Failed payload decoding
        // must answer the initial request, not an anonymous continuation header.
        let mut response = if control.continuation() {
            self.transfer
                .as_ref()
                .map(|transfer| transfer.control)
                .unwrap_or(control)
        } else {
            control
        };
        // Quiet policy is the one permitted per-chunk override. Apply it even
        // on errors, so q=2 never leaks a response into an application's input.
        if control.has(b'q') {
            response.set_number(b'q', control.number(b'q', 0).unwrap_or(0));
        }
        // No mutation occurs until every known control field has a valid type
        // and the requested medium belongs to the explicitly supported subset.
        let result = control
            .validate()
            .and_then(|()| self.action(control, payload, terminal));
        // Chunk acknowledgements belong only to the final validated transmission.
        if let Err(error) = result {
            // Quiet suppresses wire replies, not diagnosis of rejected client puts.
            tracing::debug!(event="image.command_rejected", ?response, %error);
            self.transfer = None;
            reply(&response, Err(error), &mut terminal.replies);
        }
    }

    /// Dispatches supported actions before touching source pixels.
    fn action(
        &mut self,
        control: Control,
        payload: &[u8],
        terminal: &mut Terminal,
    ) -> Result<(), ImageError> {
        match control.letter(b'a', b't')? {
            b'd' => {
                // Delete is also the protocol's explicit cancellation operation;
                // no partial image survives it, irrespective of its selector.
                self.transfer = None;
                self.delete(&control, terminal)?;
                reply(&control, Ok(()), &mut terminal.replies);
            }
            b'p' => {
                // A put references validated retained pixels and never invokes the
                // decoder. Geometry validation precedes changing its tracked handle.
                let index = self.find(&control)?;
                self.put(index, &control, terminal)?;
                reply(&control, Ok(()), &mut terminal.replies);
            }
            _ => self.transmit(control, payload)?,
        }
        Ok(())
    }

    /// Accumulates one transfer under the configured allowance, yielding a worker job.
    fn transmit(&mut self, control: Control, payload: &[u8]) -> Result<(), ImageError> {
        // The Session stops later application bytes while one job is pending.
        // Reject a second direct caller rather than replacing that job's ticket.
        if self.waiting.is_some() || self.limit == 0 {
            return Err(ImageError::NoSpace);
        }
        let more = control.number(b'm', 0)? == 1;
        let continuation = self.transfer.is_some();
        // Moving the old accumulator avoids copying a large payload at each chunk.
        // A failure drops this local owner, leaving no half-admitted transfer.
        let mut transfer = self.transfer.take().unwrap_or(Transfer {
            control,
            payload: Vec::new(),
        });
        // Continuation chunks contain only m and optional q, never fresh image metadata.
        if continuation && !control.continuation() {
            return Err(ImageError::Invalid);
        }
        if control.has(b'q') {
            transfer.control.set_number(b'q', control.number(b'q', 0)?);
        }
        // The quartet decoder validates a whole chunk before appending; its
        // aggregate bound applies before compressed bytes can grow this vector.
        base64::append(payload, more, self.limit, &mut transfer.payload)?;
        // Intermediate chunks have no success reply and no visible placement.
        // Normal VT output between chunks may still move the eventual anchor.
        if more {
            self.transfer = Some(transfer);
            return Ok(());
        }
        self.prepare_decode(transfer)
    }

    /// Final chunk validation creates a bounded request without admitting an image.
    fn prepare_decode(&mut self, transfer: Transfer) -> Result<(), ImageError> {
        // Only the final chunk becomes native work. Original dimensions and
        // action survive even when the last packet contains only m=0.
        let original = transfer.control;
        let format = original.format()?;
        let size = [original.number(b's', 0)?, original.number(b'v', 0)?];
        if format != Format::Png && self.charge(size)? > self.limit {
            return Err(ImageError::NoSpace);
        }
        // PNG's wire size after zlib expansion is independent of its decoded
        // RGBA footprint. S bounds that intermediate encoded representation.
        let uncompressed_size = original
            .has(b'S')
            .then(|| original.number(b'S', 0).map(|n| n as usize))
            .transpose()?;
        if format == Format::Png && original.has(b'o') && uncompressed_size.is_none() {
            return Err(ImageError::Invalid);
        }
        self.serial = self.serial.wrapping_add(1);
        // The ticket pairs a worker result with this exact outstanding request.
        // No placement or source table entry exists until the result commits.
        self.waiting = Some((self.serial, original));
        self.request = Some(DecodeRequest {
            ticket: self.serial,
            format,
            size,
            compressed: original.has(b'o'),
            uncompressed_size,
            payload: transfer.payload,
            limit: self.limit,
        });
        Ok(())
    }

    /// Applies one worker completion before the parser admits the next source byte.
    pub fn complete(&mut self, result: DecodeResult, terminal: &mut Terminal) {
        // Shutdown/reset can invalidate a job after the worker has accepted it.
        // Its eventual message owns only pixels and can be safely discarded.
        let Some((ticket, control)) = self.waiting else {
            return;
        };
        // A late result from an earlier generation cannot answer a newer query.
        if ticket != result.ticket {
            return;
        }
        // Release parsing before returning: the caller flushes this reply first,
        // then resumes the retained VT suffix at its next owner turn.
        self.waiting = None;
        // The completion consumes the job ticket even when decoding failed.
        self.request = None;
        let outcome = result
            .image
            .and_then(|image| self.commit(control, image, terminal));
        match outcome {
            Ok(assigned) => reply(&assigned, Ok(()), &mut terminal.replies),
            Err(error) => reply(&control, Err(error), &mut terminal.replies),
        }
    }

    /// Checks output length and placement geometry before any eviction or replacement.
    fn commit(
        &mut self,
        mut control: Control,
        image: crate::images::DecodedImage,
        terminal: &mut Terminal,
    ) -> Result<Control, ImageError> {
        // Treat worker values as untrusted too. Geometry and byte length must
        // agree before quota arithmetic or any prior source is touched.
        let bytes = crate::images::pixel_bytes(image.size, 4)?;
        if bytes != image.rgba.len() {
            return Err(ImageError::Invalid);
        }
        let charge = self.charge(image.size)?;
        // Recheck at completion: settings or the device limit may change while
        // the worker owns the payload, without blocking the Session's controls.
        if charge > self.limit {
            return Err(ImageError::NoSpace);
        }
        // Action and identity validation are already complete at this point.
        // Keeping native decoding outside this method makes commit deterministic
        // and lets property tests inject a failed decoder without any OS service.
        let action = control.letter(b'a', b't')?;
        // Queries exercise real decoding but never change source ownership,
        // recency, placements, or an existing image with the same wire ID.
        if action == b'q' {
            return Ok(control);
        }
        // Validate the display rectangle before replacing old content. Otherwise
        // a bad offset would destroy a valid image before the put could fail.
        if action == b'T' {
            self.validate_put(image.size, &control, terminal)?;
        }
        self.assign_identity(&mut control)?;
        let number = control.number(b'I', 0)?;
        let id = control.number(b'i', 0)?;
        // Admission frees space before the new immutable source becomes addressable.
        // The preflight includes replacement credit, preserving the previous source
        // when other visible placements leave insufficient room for this generation.
        self.make_room(charge, id, terminal)?;
        self.serial = self.serial.wrapping_add(1);
        // No further validation can fail for a transmit-only operation. Publish
        // the immutable source only after enough accounted bytes have been freed.
        self.images.push(Image {
            id,
            number,
            generation: self.serial,
            pixels: image,
            used: self.serial,
            charge,
        });
        // Accounting changes once per source, independent of placement count.
        // Many puts share these pixels instead of multiplying decoded memory.
        self.bytes += bytes;
        self.charged += charge;
        if action == b'T' {
            self.put(self.images.len() - 1, &control, terminal)?;
        }
        // Image-number replies include the allocated ID in addition to the supplied number.
        Ok(control)
    }

    /// Number requests allocate a fresh ID before atomic admission can retire sources.
    fn assign_identity(&self, control: &mut Control) -> Result<(), ImageError> {
        // An anonymous transmission keeps ID zero and never replaces another image.
        if control.number(b'I', 0)? != 0 {
            control.set_number(b'i', self.next_id()?);
        }
        Ok(())
    }

    /// Derives an unused wire ID; every retained source has at least four pixel bytes.
    fn next_id(&self) -> Result<u32, ImageError> {
        // The retained table is byte-bounded, so this finds a hole without a
        // second unbounded ID registry or wrapping an occupied wire identifier.
        (1..=u32::MAX)
            .find(|id| self.images.iter().all(|image| image.id != *id))
            .ok_or(ImageError::NoSpace)
    }
}

/// Quiet policy applies equally to success, scope rejection and decoder failures.
fn reply(control: &Control, result: Result<(), ImageError>, output: &mut Vec<u8>) {
    // Reply headers are bounded numeric values; never interpolate untrusted
    // protocol payload text into the application's terminal input stream.
    let quiet = control.number(b'q', 0).unwrap_or(0);
    let id = control.number(b'i', 0).unwrap_or(0);
    let number = control.number(b'I', 0).unwrap_or(0);
    // Anonymous ordinary transmissions have no acknowledgements. Image numbers
    // receive their allocated ID; q=1 suppresses only successful responses.
    if quiet == 2 || quiet == 1 && result.is_ok() || id == 0 && number == 0 {
        return;
    }
    output.extend_from_slice(b"\x1b_G");
    // Replies are a cold protocol path, not a per-byte or per-cell allocation.
    // Emit only identifiers supplied by the sender or allocated for its number.
    let mut keys = Vec::new();
    if id != 0 {
        keys.push(format!("i={id}"));
    }
    if number != 0 {
        keys.push(format!("I={number}"));
    }
    // A zero placement ID denotes anonymous instances and is omitted from
    // acknowledgements, just as a missing placement key is. The image identifier
    // remains sufficient for the sender to correlate this response.
    if let Ok(placement) = control.number(b'p', 0)
        && placement != 0
    {
        keys.push(format!("p={placement}"));
    }
    output.extend_from_slice(keys.join(",").as_bytes());
    output.push(b';');
    // Stable ASCII error codes contain no filenames, pixel bytes or host data.
    // ST terminates the response independently of later DA/OSC reply bytes.
    match result {
        Ok(()) => output.extend_from_slice(b"OK"),
        Err(error) => output.extend_from_slice(error.to_string().as_bytes()),
    }
    output.extend_from_slice(b"\x1b\\");
}
