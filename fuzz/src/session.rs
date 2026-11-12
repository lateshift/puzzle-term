//! Exercise the synchronous owner with no timers, OS services or worker threads.
//! Graphics requests complete through the same typed message used by native
//! workers. Raw pixels are decoded exactly; PNG/zlib stay unsupported here so
//! the target covers APC framing/storage without inventing a second codec.
//!
//! The fixture uses a 16×8 grid and only 64 retained history lines. Short
//! mutations can therefore reach recycling, clipping and bounded-storage paths
//! without walking the normal 10,000-line user history after each chunk.
//! Every iteration starts fresh: no prior input can supply a hidden decoder
//! ticket, optional-handler record or image placement to the next iteration.
//! Worker completion is synchronous only in this no-OS host. Production FIFO
//! admission and decode-ticket validation remain unchanged beneath that seam.
use puzzle_term::{
    Session, SessionConfig, SessionMessage,
    images::{DecodeRequest, DecodeResult, DecodedImage, Format, ImageError},
    platform::{Clock, PtySize, Services, Transport, Waker},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// Parser replies are consumed, never forwarded to a real PTY or retained.
struct Sink;
/// OS effects are discarded after the real core generates them.
impl Transport for Sink {
    /// Consume the FIFO bytes without retaining an unbounded reply transcript.
    fn write(&mut self, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    /// The pure fuzz host has no PTY to resize.
    fn resize(&mut self, _: PtySize) -> Result<(), String> {
        Ok(())
    }
    /// Closing requires no OS worker or asynchronous teardown.
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// A fuzz iteration owns scheduling; damage cannot create an OS wake or timer.
struct NoWake;
/// The iteration itself determines when the owner is called next.
impl Waker for NoWake {
    /// Damage remains observable in VT without scheduling a real presentation.
    fn request_frame(&mut self) {}
    /// No wall-clock timer can survive the input that created it.
    fn wake_at(&mut self, _: Instant) {}
}

/// Functional fuzzing drains work deterministically, independent of machine load.
struct TurnClock;
/// A feed quantum is drained by the harness, not a scheduler measurement.
impl Clock for TurnClock {
    /// Return deterministic work admission rather than machine-load-dependent splits.
    fn elapsed_since(&self, _: Instant) -> Duration {
        Duration::ZERO
    }
}

/// Constructs a bounded Session with every currently implemented optional handler.
pub fn new_session() -> Session {
    // Each input constructs a fresh owner with explicit small grid/history caps.
    // Product defaults never participate in the fuzz fixture or change here.
    let mut services = Services::new(Box::new(Sink), Box::new(NoWake));
    services.clock = Box::new(TurnClock);
    // The history bound is deliberately small so eviction is reached quickly.
    let config = SessionConfig {
        columns: 16,
        rows: 8,
        scrollback_lines: crate::HISTORY_LINES,
        scrollback_bytes: crate::HISTORY_BYTES,
        ..SessionConfig::default()
    };
    let mut session = Session::new(config, services);
    install_extra(&mut session);
    // Reuse the existing tiny image fixture allowance to reach ENOSPC/eviction
    // with short inputs, without changing the production 320-MB allowance.
    session.enable_images(4096);
    session
}

/// Drains one fuzz chunk with exact FIFO decode completion and progress checks.
pub fn drive(session: &mut Session, bytes: &[u8]) {
    let now = Instant::now();
    // Retained suffixes are fed only after any preceding decode barrier completes.
    let mut outcome = session.feed(bytes, now);
    // Every continuation must consume bytes or complete a barrier. A malformed
    // stream cannot spin the owner forever behind a consumed APC terminator.
    while outcome.pending_bytes > 0 || session.image_decode_pending() {
        let completed = if let Some(request) = session.take_image_decode_request() {
            // The real admission ticket must match; a fabricated/stale worker
            // response must not unblock a different pending transmission.
            let ticket = request.ticket;
            let image = raw(request);
            assert!(
                session
                    .apply(
                        SessionMessage::ImageDecoded(DecodeResult { ticket, image }),
                        now
                    )
                    .is_ok()
            );
            true
        } else {
            false
        };
        // Empty feed resumes accepted bytes rather than re-enqueuing them.
        outcome = session.feed(&[], now);
        assert!(
            completed || outcome.parsed_bytes > 0,
            "owner continuation made no progress"
        );
    }
    // Inspect both screens only after the accepted suffix and decoder barriers
    // are drained, exactly as a source-owner operation boundary would observe.
    crate::invariants(session.terminal());
    if let Some(images) = session.images() {
        // Admission counts real decoded bytes, not the length of a compressed input.
        assert!(images.decoded_bytes() <= images.limit());
        for image in images.images() {
            assert_eq!(
                image.pixels.rgba.len(),
                image.pixels.size[0] as usize * image.pixels.size[1] as usize * 4
            );
        }
    }
}

/// Mirrors only the native worker's trivial uncompressed RGB/RGBA conversion.
fn raw(request: DecodeRequest) -> Result<DecodedImage, ImageError> {
    // Unsupported codecs are explicit worker errors, not successful empty
    // pictures. Storage must remain unchanged and following output must resume.
    if request.compressed || request.format == Format::Png {
        return Err(ImageError::Unsupported);
    }
    let count = puzzle_term::images::pixel_bytes(request.size, 4)?;
    if count > request.limit {
        return Err(ImageError::NoSpace);
    }
    // The owned payload can become the immutable source without another copy
    // for RGBA. RGB conversion allocates only its checked admitted footprint.
    let rgba = match request.format {
        Format::Rgba if request.payload.len() == count => request.payload,
        Format::Rgb if request.payload.len() == count / 4 * 3 => {
            let mut out = Vec::with_capacity(count);
            for rgb in request.payload.chunks_exact(3) {
                out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
            out
        }
        _ => return Err(ImageError::Invalid),
    };
    // Only checked, exact-size owned bytes cross back through the worker seam.
    // Source dimensions and the byte count therefore describe the same image.
    Ok(DecodedImage {
        size: request.size,
        rgba: Arc::from(rgba),
    })
}

// Optional protocol registration is removed as one complete harness component.
include!("session/optional_items.rs");
