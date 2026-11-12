//! Session-owned decoded storage and cell-anchored placements (M15.2).
//! Pixel data commits only after worker validation. LRU admission prefers images
//! without visible placements; rejected transfers leave prior images intact.
use super::{DecodeRequest, DecodedImage, ImageError, control::Control};
use crate::vt::{Terminal, terminal::ActiveScreen, tracked::TrackedPos};

/// One decoded image has a private generation even when its wire ID is anonymous.
#[derive(Debug)]
pub struct Image {
    /// Wire ID; zero identifies anonymous transmissions that cannot be replaced by ID.
    pub id: u32,
    /// An image number may name several images; commands resolve the newest one.
    pub number: u32,
    /// Monotonic internal identity prevents stale snapshots referring to replacements.
    pub generation: u64,
    /// Immutable CPU source used again after GPU eviction or device loss.
    pub pixels: DecodedImage,
    /// Recency changes on successful put and transmission, never on failed input.
    pub(crate) used: u64,
    /// Admission cost covers decoded bytes and the exact device tile borders.
    pub(crate) charge: usize,
}

/// A placement tracks one cell through scrollback and reflow on its own screen.
#[derive(Clone, Debug)]
pub struct Placement {
    /// Private image generation resolves exactly one stored source.
    pub image: u64,
    /// Invisible prototype referenced by text; it has no physical screen location.
    pub virtual_: bool,
    /// Zero allows several placements; nonzero IDs replace the same image's placement.
    pub id: u32,
    /// Cell position lives in VT's screen-aware registry.
    pub anchor: TrackedPos,
    /// Primary and alternate screen line-ID spaces are independent.
    pub screen: ActiveScreen,
    /// Cropped source pixels, x/y/width/height after intersection with image bounds.
    pub source: [u32; 4],
    /// Physical device pixel offsets inside the anchor cell.
    pub offset: [u32; 2],
    /// Requested display size measured in cells; natural aspect is preserved by drawing.
    pub cells: [u32; 2],
    /// Original c/r request distinguishes natural pixels from aspect-fitted cells.
    pub requested_cells: [u32; 2],
    /// Kitty's signed z value selects below-background, below-text or above-text drawing.
    pub z: i32,
}

/// A transfer owns its original metadata across continuation chunks.
pub(crate) struct Transfer {
    /// Identity, placement and quiet policy belong to the initial command.
    pub control: Control,
    /// Accumulated decoded Base64 bytes stay within the Session allowance.
    pub payload: Vec<u8>,
}

/// Lazy protocol state; an image-free Session has no allocated image storage.
pub struct Store {
    /// Independently configured decoded byte allowance, also used later by GPU residency.
    pub(crate) limit: usize,
    /// Exact retained RGBA bytes; compressed bytes cannot hide decoded cost.
    pub(crate) bytes: usize,
    /// Sum of max(decoded bytes, device tile bytes) across retained sources.
    pub(crate) charged: usize,
    /// Native texture dimension limit; headless clients default to unsplit storage.
    pub(crate) texture_limit: u32,
    /// Retained immutable sources; removals also release their tracked placements.
    pub(crate) images: Vec<Image>,
    /// Screen-local instances reference generations rather than recycled wire IDs.
    pub(crate) placements: Vec<Placement>,
    /// Only one kitty chunked transmission can be in flight.
    pub(crate) transfer: Option<Transfer>,
    /// One worker job pauses later application parsing but never control messages.
    pub(crate) waiting: Option<(u64, Control)>,
    /// Worker request remains here until a platform queue accepts it.
    pub(crate) request: Option<DecodeRequest>,
    /// Internal generations and recency share this monotonic logical clock.
    pub(crate) serial: u64,
    /// Last observed RIS discards all protocol state.
    pub(crate) reset: u64,
    /// Per-screen ED 2 generation removes placements even when rows keep their IDs.
    pub(crate) clear: [u64; 2],
    /// Latest displayed row interval; None uses the live bottom for standalone VT clients.
    pub(crate) viewport: Option<(ActiveScreen, usize, usize)>,
}

/// Core ownership has no worker, locks or asynchronous runtime.
impl Store {
    /// Enables graphics with the caller's validated allowance; zero rejects transmissions.
    pub fn new(limit: usize, terminal: &Terminal) -> Self {
        Self {
            limit,
            bytes: 0,
            charged: 0,
            texture_limit: u32::MAX,
            images: Vec::new(),
            placements: Vec::new(),
            transfer: None,
            waiting: None,
            request: None,
            serial: 0,
            reset: terminal.reset_epoch,
            clear: terminal.image_clear_epochs(),
            viewport: None,
        }
    }

    /// Updates the viewport sampled by the Session, including fractional edge rows.
    pub fn set_viewport(&mut self, terminal: &Terminal, top: usize) {
        // The extra row covers the fractional smooth-scroll edge. Eviction must
        // not discard a source while part of its bottom/top row is still shown.
        self.viewport = Some((
            terminal.active,
            top,
            top.saturating_add(terminal.size().1 + 1),
        ));
    }

    /// Returns the displayed interval in the active ring's retained-row coordinates.
    pub(crate) fn visible_rows(&self, terminal: &Terminal) -> (usize, usize) {
        self.viewport
            .filter(|(screen, _, _)| *screen == terminal.active)
            .map(|(_, top, end)| (top, end))
            .unwrap_or_else(|| {
                (
                    terminal.screen().ring.scrollback_len(),
                    terminal.screen().ring.scrollback_len() + terminal.size().1,
                )
            })
    }

    /// Configured decoded allowance is also the independently accounted GPU limit.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Shrinking or disabling frees sources by the same visibility/recency rule.
    pub fn set_limit(&mut self, limit: usize, terminal: &mut Terminal) {
        self.limit = limit;
        // Shrinks use the same policy as admission. Zero therefore releases all
        // retained sources and their registry handles rather than hiding them.
        while self.charged > limit {
            let index = self
                .images
                .iter()
                .enumerate()
                .min_by_key(|(_, image)| (self.visible(image.generation, terminal), image.used))
                .map(|(i, _)| i);
            if let Some(index) = index {
                self.remove_image(index, terminal);
            } else {
                break;
            }
        }
        if limit == 0 {
            self.transfer = None;
        }
    }

    /// Exact decoded accounting is visible to tests and native diagnostic reporting.
    pub fn decoded_bytes(&self) -> usize {
        self.bytes
    }

    /// Immutable stored images supply pixels only to visible frame publications.
    pub fn images(&self) -> &[Image] {
        &self.images
    }

    /// Retained placements include history; visibility is resolved against the viewport.
    pub fn placements(&self) -> &[Placement] {
        &self.placements
    }

    /// Application parsing pauses until the worker result is applied in source order.
    pub fn decode_pending(&self) -> bool {
        self.waiting.is_some()
    }

    /// Transfers ownership once; a full native queue retains this request on its owner.
    pub fn take_request(&mut self) -> Option<DecodeRequest> {
        self.request.take()
    }

    /// Native queue backpressure returns the same value without dropping a valid image.
    pub fn return_request(&mut self, request: DecodeRequest) {
        self.request = Some(request);
    }

    /// Removes every placement of an evicted/replaced source and releases its VT handles.
    pub(crate) fn remove_image(&mut self, index: usize, terminal: &mut Terminal) {
        // CPU pixels are immutable: a sealed publication may retain its own Arc
        // until the renderer replaces it, but the Session releases its ownership.
        let image = self.images.remove(index);
        // Placements carry no independent pixel cost; their removal must not
        // subtract the same immutable source once for every displayed instance.
        self.bytes -= image.pixels.rgba.len();
        self.charged -= image.charge;
        self.placements.retain(|placement| {
            if placement.image != image.generation {
                return true;
            }
            terminal.tracked.release(placement.anchor);
            false
        });
    }

    /// Resolve explicit IDs or the most recent image having a requested number.
    pub(crate) fn find(&self, control: &Control) -> Result<usize, ImageError> {
        let id = control.number(b'i', 0)?;
        let number = control.number(b'I', 0)?;
        self.images
            .iter()
            .rposition(|image| {
                if number != 0 {
                    image.number == number
                } else {
                    id != 0 && image.id == id
                }
            })
            .ok_or(ImageError::NotFound)
    }
}
