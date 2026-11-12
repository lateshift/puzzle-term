//! Parsed loaded faces have bounded ownership and survive unrelated glyph jobs.
//!
//! The platform calls this owner from its worker, or lends it nonblockingly
//! for an admitted draw. A missing face or hinter can never be created inline.
use super::{Ink, Job, Outline};
use crate::face::FontFace;

/// Resident parsing state for immutable bytes already supplied to a FontSet.
#[derive(Default)]
pub struct Faces {
    /// LRU order bounds parsed metadata independently of glyph/atlas counts.
    entries: Vec<Entry>,
    /// Procedural calls have no mapping but still need measured admission.
    procedural_cost: super::cost::RecentCost,
}
/// A collection face and variation weight require independent parsed state.
struct Entry {
    /// Pointer identity is stable while this owner retains the shared FontData.
    identity: (usize, u32, u16),
    /// The loaded owner includes reusable shaping and ppem hinting state.
    face: FontFace,
    /// Hinted and unhinted calls retain independent recent preparation evidence.
    costs: [super::cost::RecentCost; 2],
}
impl Faces {
    /// Creates first-use parsing state on the worker before measuring reusable work.
    /// The shared bytes remain owned by the entry, so the pointer identity stays valid.
    pub fn load(&mut self, job: &Job) -> Result<(), String> {
        // Creating engines is unpredictable first-use work and belongs to the worker.
        if job.data.is_some() && !self.entries.iter().any(|e| e.identity == identity(job)) {
            // The entry retains bytes and parsing together; neither view can outlive its font.
            self.entries.push(Entry {
                identity: identity(job),
                costs: Default::default(),
                face: FontFace::new(job.data.clone().ok_or("font bytes absent")?, job.weight)?,
            });
        }
        Ok(())
    }
    /// Returns true only after all first-use face and hinter work has completed.
    pub fn ready(&self, job: &Job) -> bool {
        // Cell-generated shapes have no OpenType parsing or mapping dependency.
        if job.data.is_none() {
            return true;
        }
        // Device size is not face identity; hinted state is checked separately.
        let key = identity(job);
        self.entries
            .iter()
            .find(|e| e.identity == key)
            .is_some_and(|entry| {
                // A loaded file is insufficient when its requested hinter is still cold.
                entry
                    .face
                    .hint_ready(job.placement.key.hint_ppem, job.placement.key.hint_mode)
                    // COLR traversal has variable graph complexity and stays off-frame.
                    && !entry.face.has_colr()
            })
    }
    /// Records preparation outside the core's clockless font ownership logic.
    pub fn record(&mut self, job: &Job, cost: std::time::Duration) {
        // Cell-generated shapes have no OpenType parsing or mapping dependency.
        if job.data.is_none() {
            self.procedural_cost.record(cost);
        } else if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.identity == identity(job))
        {
            entry.costs[usize::from(job.placement.key.hint_ppem != 0)].record(cost);
        }
    }
    /// A ready face must also have a measured call before inline admission.
    pub fn predicted(&self, job: &Job) -> Option<std::time::Duration> {
        // Unknown parsing cost cannot be learned by spending the refresh margin.
        if !self.ready(job) {
            return None;
        }
        let cost = if job.data.is_none() {
            self.procedural_cost.p95()
        } else {
            self.entries
                .iter()
                .find(|e| e.identity == identity(job))?
                .costs[usize::from(job.placement.key.hint_ppem != 0)]
            .p95()
        };
        // No successful observation means the worker must establish readiness first.
        cost
    }
    /// Prepares one complete shape, reusing a parsed face and variation instance.
    /// Only the background caller may reach a missing face or hinter here.
    pub fn prepare(&mut self, job: &Job) -> Result<Option<Ink>, String> {
        // Cell-generated shapes have no OpenType parsing or mapping dependency.
        if job.data.is_none() {
            // Procedural cell geometry has no file, tables or first-use font setup.
            // The packed cell instance contains the geometry dimensions, independent of fonts.
            let cell = crate::builtin::BuiltinCell::unpack(job.placement.key.instance)?;
            let key = job.placement.key;
            // Shared diagnostic components and complete builtin paths use distinct namespaces.
            let path = if key.face == crate::builtin::TOFU_COMPONENT_FACE {
                // A positioned hex digit is reused by all provisional scalars with that digit.
                cell.tofu_component(key.glyph_id)?.path
            } else if key.face == crate::builtin::TOFU_FACE {
                // Explicit unsupported diagnostics still have the original complete path.
                cell.tofu(key.glyph_id).path
            } else {
                // Invalid scalars report an input error before procedural generation.
                cell.ink(char::from_u32(key.glyph_id).ok_or("invalid builtin scalar")?)
                    .ok_or("builtin ink absent")?
                    .path
            };
            // Conversion and band building transfer the same geometry later drawn.
            return Outline::new(path).map(|outline| outline.map(Ink::Outline));
        }
        // Device size is not face identity; hinted state is checked separately.
        let key = identity(job);
        // A worker may load here; inline callers already passed the ready check.
        self.load(job)?;
        // Locate the exact collection face and variable weight selected for this job.
        let slot = self
            .entries
            .iter()
            .position(|e| e.identity == key)
            .ok_or("loaded face absent")?;
        // Taking one owner avoids mutable aliasing while skrifa updates hinter state.
        let mut entry = self.entries.remove(slot);
        // Hint suffixes belong to this placement rather than the parsed-face identity.
        let p = &job.placement;
        // Authored color is preferred; an ordinary outline needs exactly one preparation.
        let result = match entry.face.color_layers(p.key.glyph_id, 0xffffffff) {
            Ok(Some(color)) => Ok(Some(Ink::Color(color))),
            // Absent COLR data is normal and falls through to the outline tables.
            Ok(None) => entry
                .face
                .outline(p.key.glyph_id, p.key.hint_ppem, p.key.hint_mode)
                .and_then(Outline::new)
                .map(|outline| outline.map(Ink::Outline)),
            Err(error) => Err(error),
        };
        // Most recently used parsing moves to the end without cloning its engines.
        self.entries.push(entry);
        // Use the native resident-face ceilings, independent of any workload glyph count.
        // The bytes are shared, so this is retained ownership, not an additional heap copy.
        while self.entries.len() > 32 || self.bytes() > 512 * 1024 * 1024 {
            // The oldest byte owner is released only when a bounded resident limit is hit.
            self.entries.remove(0);
        }
        result
    }
    /// Counts shared byte owners once, even when primary style faces share a file.
    fn bytes(&self) -> usize {
        // Style instances of one file share an Arc and must not be charged repeatedly.
        self.entries
            .iter()
            .enumerate()
            .filter(|(i, entry)| {
                // A bounded linear scan finds an earlier owner of the same byte allocation.
                !self.entries[..*i]
                    .iter()
                    .any(|prior| prior.identity.0 == entry.identity.0)
            })
            // Mapped bytes describe virtual ownership, not private resident memory.
            .map(|(_, entry)| entry.face.data().bytes().len())
            .sum()
    }
}
/// The byte allocation remains alive in both the frame FontSet and parsed owner.
fn identity(job: &Job) -> (usize, u32, u16) {
    // Pointer identity stays valid because every resident entry owns these immutable bytes.
    (
        job.data.as_ref().map_or(0, |d| d.bytes().as_ptr() as usize),
        job.data.as_ref().map_or(0, |d| d.index()),
        job.weight,
    )
}
