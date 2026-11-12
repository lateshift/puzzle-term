//! Applies owned worker results independently of current viewport contents.
//!
//! A frame boundary admits one ready result at a time under its time allowance.
//! Permanent scalar facts survive temporary registry retirement; sized
//! ink retains its exact producer placement until the host atlas takes ownership.
//!
//! Structural fact-only probes own no final ink; actual results may restore retired
//! scalar metadata. Composed identities require their original entry to survive.
//! Failed preparation clears its choice once, preventing perpetual retry loops.
//! Atlas transfer drains retained variants before the next publication batch.
//! Registry pressure never changes the facts or already prepared atlas addresses.
use super::*;

/// Completion publication and bounded metadata replacement share one owner.
impl Deferred {
    /// Applies one already completed unit; the host decides how long to keep publishing.
    pub(super) fn changed(&mut self) -> bool {
        let Some(done) = self.driver.poll() else {
            return false;
        };
        self.apply_completion(done)
    }

    /// Restores exact ownership before moving one result into the ordinary atlas lane.
    fn apply_completion(&mut self, done: Completion) -> bool {
        let mut changed = false;
        // Recreate the borrowed lookup tuple from the producer-owned full identity.
        // Equality below is authoritative even if two clusters share a hash value.
        let r = ClusterRequest {
            cluster: &done.job.key.cluster,
            style: done.job.key.style,
            presentation: done.job.key.presentation,
        };
        // A result remains a valid scalar fact even when its temporary entry retired.
        // Sized failures invalidate that source choice instead of retrying forever.
        let covered =
            done.covered && (done.job.placement.is_none() || matches!(done.result, Ok(Some(_))));
        changed |= self.facts.set(r, covered);
        if done.job.placement.is_some()
            && self.facts.get(r).is_some()
            && self
                .entries
                .get(&Self::hash(r))
                .is_none_or(|e| e.key != done.job.key)
        {
            // Reversible scalar identity still proves this completion's owner after
            // metadata pressure. Retain its ink for this frame's atlas transfer.
            self.restore(r, Self::hash(r));
        }
        if let Some(e) = self.entries.get_mut(&Self::hash(r))
            && e.key == done.job.key
        {
            // Evicted requests cannot redirect stale completion into a replacement key.
            if let Some(p) = done.job.placement {
                // The first real draw establishes the same authoritative fact as
                // a legacy coverage-only completion, including composed clusters.
                e.covered = Some(covered);
                // A rejected candidate remains tofu, rather than aborting the whole frame.
                match done.result {
                    Ok(Some(ink)) => {
                        e.ink.insert(Size::of(&p), Some(ink));
                        self.ready.track(&mut self.entries, Self::hash(r), p);
                    }
                    // Failed ink retires this source choice once, avoiding endless retries.
                    _ => {
                        e.covered = Some(false);
                        e.ink.remove(&Size::of(&p));
                    }
                }
            } else {
                // A negative answer is cached just like a positive, without forcing
                // another discovery pass when the next row contains this cluster.
                e.covered = Some(done.covered);
            }
            changed = true;
        }
        changed
    }

    /// Recreates bounded ink metadata while retaining permanent scalar coverage.
    pub(super) fn restore(&mut self, r: ClusterRequest<'_>, hash: u64) {
        if self.entries.remove(&hash).is_some() {
            // A hash collision replaces ownership, never appends another FIFO identity.
            self.order.retain(|old| *old != hash);
            self.ready.retire(hash);
        }
        if !self.make_room() {
            // No new job is submitted when all bounded metadata belongs to accepted work.
            return;
        }
        self.entries.insert(
            hash,
            Entry {
                key: Key {
                    cluster: r.cluster.into(),
                    style: r.style,
                    presentation: r.presentation,
                },
                covered: self.facts.get(r),
                ink: FxHashMap::default(),
            },
        );
        self.order.push_back(hash);
    }

    /// Retires one temporary request before admitting a new or restored identity.
    pub(super) fn make_room(&mut self) -> bool {
        // Coverage pages and downstream atlas ink deliberately outlive this registry.
        // Only owned variants awaiting their first transfer retire with the entry.
        if self.entries.len() < 4096 {
            return true;
        }
        // Accepted preparation cannot be forgotten: doing so would validate the same
        // glyph twice and discard the first job's pending ownership under Unicode churn.
        let slot = self.order.iter().position(|hash| {
            self.entries.get(hash).is_none_or(|entry| {
                !entry.ink.values().any(Option::is_none) && entry.covered.is_some()
                    || entry.ink.is_empty() && self.driver.validates_during_prepare()
            })
        });
        let Some(old) = slot.and_then(|slot| self.order.remove(slot)) else {
            return false;
        };
        // Completed facts outlive retirement; only their temporary request metadata leaves.
        self.entries.remove(&old);
        self.ready.retire(old);
        true
    }
}
