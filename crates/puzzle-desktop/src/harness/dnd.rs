//! Synthetic native drag commands cover the full host contract without OS APIs.
//! The scenario supplies presented geometry, typed MIME offers and actual drop
//! bytes. Parser replies are checked byte-for-byte; no native acceptance is faked.
//!
//! The window toolkit is intentionally not the protocol test oracle.
//! Synthetic commands describe host observations, including positions and MIME offers.
//! They exercise enter, changed offers, movement, rejection, Drop and leave deterministically.
//! A recording transport retains the exact reply sequence rather than parsing it loosely.
//! The fixture Session owns all state and is called synchronously on this one host thread.
//! The recorder's lock belongs to the test transport, never to the platform-neutral core.
//!
//! The scenario uses fixed logical cell geometry and explicit DPR from its script.
//! A presented-region message installs that geometry through the real owner input seam.
//! This covers padding/scale conversion without relying on native window placement.
//! No GPU screenshot can substitute for the protocol's zero-based coordinate assertion.
//! The containing scenario still owns the usual native renderer for existing commands.
//! Drag state is created only by drag-start so ordinary scenarios have no extra work.
//!
//! Commands with unrelated prefixes fall through to the existing scenario dispatcher.
//! A drag-prefixed typo is an error instead of silently becoming an ignored command.
//! Quoted byte strings use the same escape decoder as feed, including binary zeroes.
//! MIME alternatives retain their order, because the program reads them by index.
//! Each offered alternative receives the explicit test payload when Drop is injected.
//! Movement has empty bytes even if the same names were offered in an earlier drop.
//!
//! The fixture copies registration generations from host events instead of guessing 1.
//! That makes unregister/re-register scenarios meaningful and protects stale callbacks.
//! Effects are collected only after Session finishes the corresponding owner turn.
//! Reply assertions drain all bytes so a later request cannot conceal earlier output.
//! A mismatch reports both actual and expected byte vectors with scenario line context.
//! Effect assertions distinguish enabled registration, disabled registration and acceptance.
//! Conclusion is independently asserted rather than inferred from a successful data read.
//!
//! Host input is trusted in precisely the same way as the native C fixture's gestures.
//! It supplies captured values, never a filesystem or clipboard capability.
//! Unregistered and stale inputs remain inert inside the protocol owner.
//! The full flow is tested independently of the harness's file-only OS adapter.
//! This separation preserves the user-approved harness limitation without weakening coverage.
//! No native dragging bridge or expanded puzzle-sys safety exception is needed.
//! The real macOS host consumes the same registration and acceptance callback contract.
//! Its live behavior remains a human review item at M18.9.

use super::{Scenario, preview, scroll::Manual};
use puzzle_term::{
    FrameResult, HostEvent, Session, SessionConfig, SessionMessage,
    clipboard::MimeData,
    dnd::{Effect, Input, Kind},
    platform::{PtySize, Services, Transport},
};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};
/// Reply storage is a test host service, never shared inside the core.
struct Recorder(Arc<Mutex<Vec<u8>>>);
impl Transport for Recorder {
    /// Preserve every write in the owner's FIFO order.
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "drag recorder poisoned")?
            .extend_from_slice(bytes);
        Ok(())
    }
    /// Synthetic geometry has no operating-system child.
    fn resize(&mut self, _: PtySize) -> Result<(), String> {
        Ok(())
    }
    /// Fixture closure owns no external process.
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}
/// Allocated only by drag-start, independent of normal rendering fixtures.
pub(super) struct Scene {
    /// The sole parser and protocol state owner.
    session: Session,
    /// Host copies registration from its public callback queue.
    generation: u64,
    /// Exact transport bytes are drained by expect-drag-reply.
    replies: Arc<Mutex<Vec<u8>>>,
    /// Retain semantic callbacks until independently asserted.
    effects: Vec<Effect>,
}
/// Recognizes only drag-prefixed commands, leaving other scenarios untouched.
pub(super) fn command(state: &mut Scenario, line: &str) -> Option<Result<(), String>> {
    // Only the leading command token selects this family; quoted payloads stay opaque.
    let name = line.split_whitespace().next()?;
    if !name.starts_with("drag-") && !name.starts_with("expect-drag-") {
        return None;
    }
    Some(run(state, line, name))
}
/// Commands enter through Session messages; assertions never read private VT state.
fn run(state: &mut Scenario, line: &str, name: &str) -> Result<(), String> {
    // Replacing a fixture intentionally begins a completely independent native surface.
    if name == "drag-start" {
        state.drag_scene = Some(Scene::new(state.dpr)?);
        return Ok(());
    }
    // A missing fixture is a script error rather than an implicit registration.
    let scene = state.drag_scene.as_mut().ok_or("drag-start required")?;
    let now = Instant::now();
    match name {
        "drag-feed" => {
            // Feed untrusted bytes through the real parser, not a private protocol helper.
            scene.session.feed(&quoted_bytes(line)?, now);
        }
        "drag-enter" | "drag-move" | "drag-drop" | "drag-leave" => scene.input(line, name, now)?,
        "expect-drag-reply" => {
            // Decode escaped binary strings with the same rules as ordinary scenario feed.
            let expected = quoted_bytes(line)?;
            // Taking the recorder buffer makes FIFO boundaries explicit between assertions.
            let actual =
                std::mem::take(&mut *scene.replies.lock().map_err(|_| "drag recorder poisoned")?);
            if actual != expected {
                return Err(format!("drag reply mismatch: {actual:?} != {expected:?}"));
            }
        }
        "expect-drag-effect" => {
            let wanted = line
                .split_whitespace()
                .nth(1)
                .ok_or("effect name missing")?;
            let actual = scene.effects.pop().ok_or("missing drop effect")?;
            // Registration None is a distinct lifecycle observation from enabling a target.
            let tag = match actual {
                Effect::Registration { mimes: Some(_), .. } => "registration",
                Effect::Registration { mimes: None, .. } => "unregister",
                Effect::Acceptance { .. } => "acceptance",
                Effect::Concluded { .. } => "concluded",
            };
            if tag != wanted {
                return Err(format!("drop effect {tag} != {wanted}"));
            }
        }
        _ => return Err(format!("unknown drag command {name}")),
    }
    // Host callbacks become observable only after the owner turn has completed.
    scene.collect();
    Ok(())
}
impl Scene {
    /// Install explicit logical cells and scale as a synthetic presented host frame.
    fn new(dpr: f32) -> Result<Self, String> {
        let replies = Arc::new(Mutex::new(Vec::new()));
        let mut session = Session::new(
            SessionConfig::default(),
            Services::new(Box::new(Recorder(replies.clone())), Box::new(Manual)),
        );
        // Install the normal host queue before feeding any registration output.
        session.enable_host_events()?;
        let geometry = puzzle_term::overlay::geometry::TextGeometry {
            cell: [8.0, 16.0],
            inset: 4.0,
            dpr,
            columns: 80,
            ..Default::default()
        };
        let mut regions = puzzle_term::overlay::regions::PresentedRegions::default();
        // Presented geometry, not desired resize geometry, drives the later point mapping.
        regions.capture(&geometry, std::iter::empty());
        session.apply(
            SessionMessage::FrameResult(FrameResult::Presented(regions)),
            Instant::now(),
        )?;
        Ok(Self {
            session,
            generation: 0,
            replies,
            effects: Vec::new(),
        })
    }
    /// Copy semantic host callbacks, retaining their generation rather than guessing one.
    fn collect(&mut self) {
        while let Some(event) = self.session.take_event() {
            if let HostEvent::Drop(effect) = event {
                // Generations are callback data; never assume a reset returns the counter to one.
                if let Effect::Registration { generation, .. } = &effect {
                    self.generation = *generation;
                }
                self.effects.push(effect);
            }
        }
    }
    /// Offers comma-separated MIME names; a drop uses the quoted payload for each offered representation.
    fn input(&mut self, line: &str, name: &str, now: Instant) -> Result<(), String> {
        let kind = match name {
            "drag-leave" => Kind::Leave,
            "drag-drop" => Kind::Drop,
            _ => Kind::Move,
        };
        // Names are comma-separated to keep a single unambiguous command field.
        let mut fields = line.split_whitespace().skip(1);
        let mut input = Input {
            generation: self.generation,
            kind,
            position: [0.0; 2],
            operations: 1,
            items: Vec::new(),
        };
        if kind != Kind::Leave {
            for coordinate in &mut input.position {
                *coordinate = fields
                    .next()
                    .ok_or("drag point missing")?
                    .parse()
                    .map_err(|_| "invalid drag point")?;
            }
            input.operations = fields
                .next()
                .ok_or("operations missing")?
                .parse()
                .map_err(|_| "invalid drag operations")?;
            let mimes = fields.next().ok_or("MIME list missing")?;
            // Offers never contain bytes, so the fixture cannot grant reads through hover.
            let data = if kind == Kind::Drop {
                quoted_bytes(line)?
            } else {
                Vec::new()
            };
            for mime in mimes.split(',') {
                input.items.push(MimeData {
                    mime: mime.into(),
                    data: data.clone().into(),
                });
            }
        }
        // This is the same message shape queued by Surface and puzzle_surface_drop.
        self.session.apply(SessionMessage::Drop(input), now)
    }
}

/// Specialized model fixtures own their prefixes without enlarging the base dispatcher.
impl Scenario {
    /// Short-circuit after the first matching family so input is applied exactly once.
    pub(super) fn model_command(&mut self, line: &str) -> Option<Result<(), String>> {
        if let Some(result) = super::extra_command(self, line) {
            return Some(result);
        }
        command(self, line)
            .or_else(|| super::search::command(self, line))
            .or_else(|| super::drawn::command(self, line))
    }
}

/// Reuses the established escape decoder after this family validated the command token.
fn quoted_bytes(line: &str) -> Result<Vec<u8>, String> {
    // Only the quoted suffix is data: drag-drop also has geometry and MIME fields.
    let offset = line.find('"').ok_or("drag payload requires a quote")?;
    preview::feed_bytes(&format!("feed {}", &line[offset..]))
}
