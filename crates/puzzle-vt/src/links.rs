//! Generation-checked OSC 8 link storage (§6.11, §16).
//!
//! Cells keep a small handle rather than copying a URI into every row.
//! Replacing a slot increments its generation, so a stale handle cannot
//! resolve to an unrelated new link after bounded table eviction. The
//! application may show an unsupported URI on hover, but opening it is a
//! separate policy action with an explicit scheme allowlist.

/// Opaque link-table handle stored in sparse cell extras.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkId(pub u32);

/// A single retained URI and optional application grouping identifier.
#[derive(Clone, Debug)]
struct LinkEntry {
    /// Increments when this table slot is recycled.
    generation: u32,
    /// The bounded hyperlink target, never opened by puzzle-vt itself.
    uri: String,
    /// OSC 8's `id=` joins logically continuous link segments.
    group: Option<String>,
}

/// Bounded round-robin store for up to 4096 link targets.
#[derive(Clone, Debug, Default)]
pub struct LinkTable {
    /// Slots grow only until the §16 ceiling, then old entries are reused.
    entries: Vec<LinkEntry>,
    /// The next slot to replace when all 4096 slots are occupied.
    next: usize,
}

impl LinkTable {
    /// Copies frame metadata while preserving every warmed backing allocation.
    /// Derived Clone replaces nested buffers; detached provider snapshots reuse them.
    pub fn snapshot_from(&mut self, source: &Self) {
        // Only a new metadata high-water mark can grow these owned buffers (P1).
        self.entries.truncate(source.entries.len());
        for (index, entry) in source.entries.iter().enumerate() {
            if let Some(target) = self.entries.get_mut(index) {
                target.generation = entry.generation;
                target.uri.clone_from(&entry.uri);
                target.group.clone_from(&entry.group);
            } else {
                self.entries.push(entry.clone());
            }
        }
        self.next = source.next;
    }

    /// Reports retained slots for §16 resource-limit checks and diagnostics.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Reports whether the table has no stored URI targets.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Inserts a bounded URI, rejecting an overlong hostile OSC 8 payload.
    pub fn insert(&mut self, uri: &str, group: Option<&str>) -> Option<LinkId> {
        // URI length is checked in bytes because the OSC parser and the
        // protocol's memory ceiling measure encoded bytes, not characters.
        if uri.len() > 2048 {
            return None;
        }
        // Reuse an identical target/group pair so adjacent spans share one
        // handle without consuming extra slots during repeated OSC 8 starts.
        if let Some((index, entry)) = self
            .entries
            .iter()
            .enumerate()
            .find(|(_, entry)| entry.uri == uri && entry.group.as_deref() == group)
        {
            return Some(LinkId((entry.generation << 12) | index as u32));
        }
        let index = if self.entries.len() < 4096 {
            // Grow only until the fixed table ceiling. Old rows then retain
            // small handles instead of extending this allocation forever.
            let index = self.entries.len();
            self.entries.push(LinkEntry {
                generation: 0,
                uri: uri.into(),
                group: group.map(str::to_owned),
            });
            index
        } else {
            // A recycled slot gets a new generation before its target changes;
            // cells still carrying the old handle then resolve to no link.
            let index = self.next;
            let old = &mut self.entries[index];
            old.generation = old.generation.wrapping_add(1) & 0x000f_ffff;
            old.uri.clear();
            old.uri.push_str(uri);
            // OSC 8 grouping belongs to the target entry, so replacement
            // must clear it even when the next link has no id= parameter.
            old.group = group.map(str::to_owned);
            self.next = (self.next + 1) % 4096;
            index
        };
        Some(LinkId(
            (self.entries[index].generation << 12) | index as u32,
        ))
    }

    /// Returns a URI only when the handle still names that slot generation.
    #[must_use]
    pub fn resolve(&self, id: LinkId) -> Option<&str> {
        // The low twelve bits address exactly 4096 entries. The remaining
        // bits protect against opening a newly recycled target from old ink.
        let index = (id.0 & 0xfff) as usize;
        let generation = id.0 >> 12;
        self.entries
            .get(index)
            .filter(|entry| entry.generation == generation)
            .map(|entry| entry.uri.as_str())
    }

    /// Tests generation-checked OSC grouping; ungrouped links match only themselves.
    pub fn same_group(&self, a: LinkId, b: LinkId) -> bool {
        if self.resolve(a).is_none() || self.resolve(b).is_none() {
            return false;
        }
        // An application's explicit id joins segments even when their URIs differ.
        let first = &self.entries[(a.0 & 0xfff) as usize];
        let second = &self.entries[(b.0 & 0xfff) as usize];
        // Ungrouped handles remain independent even if their displayed URI matches.
        a == b
            || first
                .group
                .as_ref()
                .is_some_and(|group| second.group.as_ref() == Some(group))
    }

    /// Checks the explicit launch-scheme policy without opening anything.
    #[must_use]
    pub fn openable(uri: &str, desktop: bool) -> bool {
        // Opening is a platform policy decision after lookup. In particular,
        // file links cannot cross into the browser's page origin, and script
        // or data schemes must never become navigation targets (design §16).
        uri.starts_with("http://")
            || uri.starts_with("https://")
            || uri.starts_with("mailto:")
            || (desktop && uri.starts_with("file://"))
    }
}

#[cfg(test)]
#[path = "links/tests.rs"]
mod tests;
