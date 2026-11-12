//! Permanent scalar coverage uses lazy two-bit pages instead of evictable strings.
//!
//! Each source has four styles and two presentation policies per Unicode scalar.
//! The complete table is bounded at 2,228,224 bytes; only touched 512-scalar pages
//! allocate. Whole composed clusters retain the separate 4,096-entry allowance.
use super::ClusterRequest;

/// Completed facts outlive cold preparation metadata and atlas descriptors.
#[derive(Default)]
pub(super) struct Facts {
    /// A missing page represents unknown coverage without allocating storage.
    pages: Vec<Option<Box<[u8; 1024]>>>,
}

/// Borrowed scalar lookups allocate nothing, including after a full Unicode cycle.
impl Facts {
    /// Returns only completed answers; unknown and composed clusters remain deferred.
    pub(super) fn get(&self, request: ClusterRequest<'_>) -> Option<bool> {
        // Only a complete one-scalar request has a reversible page address.
        let (page, byte, shift) = position(request)?;
        // Read two appearance-specific bits; absent pages remain unknown.
        let bits = self.pages.get(page)?.as_ref()?[byte] >> shift & 3;
        match bits {
            1 => Some(false),
            2 => Some(true),
            _ => None,
        }
    }

    /// Records validated coverage once; style and presentation never share answers.
    pub(super) fn set(&mut self, request: ClusterRequest<'_>, covered: bool) -> bool {
        let Some((page, byte, shift)) = position(request) else {
            return false;
        };
        let changed = self.get(request) != Some(covered);
        if self.pages.is_empty() {
            // Unicode's fixed scalar limit bounds directory memory independently of input.
            self.pages.resize_with(0x110000 / 512, || None);
        }
        let data = self.pages[page].get_or_insert_with(|| Box::new([0; 1024]));
        // Two bits distinguish unknown, missing and covered without a retained UTF-8 copy.
        data[byte] = (data[byte] & !(3 << shift)) | ((1 + u8::from(covered)) << shift);
        changed
    }
}

/// Maps exactly one scalar's eight appearance combinations into bounded page storage.
fn position(request: ClusterRequest<'_>) -> Option<(usize, usize, usize)> {
    // Composed clusters retain whole-string validation in the bounded metadata cache.
    let mut chars = request.cluster.chars();
    let scalar = chars.next()? as usize;
    if chars.next().is_some() {
        return None;
    }
    let policy = usize::from(request.presentation == super::Presentation::Emoji);
    // Eight slots per scalar separate four font styles and two presentation policies.
    let slot = scalar * 8 + request.style as usize * 2 + policy;
    Some((slot / 4096, slot % 4096 / 4, slot % 4 * 2))
}
