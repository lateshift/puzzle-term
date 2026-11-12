//! Presented region tables prevent input from observing a future frame's layout.
//!
//! Tables contain only region/scissor commands, never glyphs or text strings.
//! Copying happens at presentation; a failed or skipped build cannot change
//! the Session's hit targets. Painter order and clipping match the draw pass.
//!
//! Region-only lists keep clip push/pop order within each painter slot. Removing
//! an unrelated text or shape command cannot alter a region's active scissors.
//! Registration identities are monotonic, so their order remains the same even
//! when a provider is removed. Geometry is copied with capacity reuse; the model
//! never borrows the renderer's live placement vectors.
//!
//! A captured pointer drag is routed by the model registry rather than this
//! picker. Picking constrains only fresh presses and hover, allowing a captured
//! thumb to continue outside the grid and surface until its paired release.
use crate::{
    draw::{CursorIcon, DrawList, Primitive, RegionId},
    geometry::TextGeometry,
    registry::{ProviderId, hit},
};

/// Immutable-by-ownership input table returned with a successful presentation.
#[derive(Default)]
pub struct PresentedRegions {
    /// Only commands needed to reproduce painter-order and nested scissors.
    lists: Vec<(ProviderId, DrawList)>,
    /// Regions and anchors must resolve using this presented frame's geometry.
    geometry: TextGeometry,
}
/// Capture does not expose the mutable view registry to the interaction owner.
impl PresentedRegions {
    /// Copies the visible hit primitives, retaining caller-owned capacities.
    pub fn capture<'a>(
        &mut self,
        geometry: &TextGeometry,
        lists: impl Iterator<Item = (ProviderId, &'a DrawList)>,
    ) {
        // Region coordinates and scissors belong to this exact presentation.
        self.geometry.clone_from(geometry);
        let mut count = 0;
        for (id, source) in lists {
            if self.lists.len() <= count {
                self.lists.push((id, DrawList::default()));
            }
            // Reuse the receiving table rather than allocating a new list per frame.
            let target = &mut self.lists[count];
            target.0 = id;
            target.1.draws.clear();
            target.1.draws.extend(
                source
                    .draws
                    .iter()
                    .filter(|draw| {
                        matches!(
                            draw.primitive,
                            Primitive::Region(..) | Primitive::ClipPush(_) | Primitive::ClipPop
                        )
                    })
                    .cloned(),
            );
            count += 1;
        }
        // Removed providers must immediately lose their old presented hit targets.
        self.lists.truncate(count);
    }
    /// Uses presented geometry, even when the renderer has already built a newer frame.
    pub fn hit(&self, point: [f32; 2]) -> Option<(ProviderId, RegionId, CursorIcon)> {
        self.lists
            .iter()
            .flat_map(|(id, list)| {
                list.draws.iter().enumerate().filter_map(move |(at, draw)| {
                    let Primitive::Region(rect, region, cursor) = draw.primitive else {
                        return None;
                    };
                    // Anchors and clipping use the captured geometry, never a newer view.
                    let picked = hit::placed(rect, draw, &self.geometry)?.contains(point)
                        && hit::unclipped(list, at, &self.geometry, point);
                    picked.then_some((draw.slot, id.0, at, *id, region, cursor))
                })
            })
            // Painter priority is slot, then registration, then retained command order.
            .max_by_key(|(slot, id, at, ..)| (*slot, *id, *at))
            .map(|(_, _, _, id, region, cursor)| (id, region, cursor))
    }
    /// The model's latest geometry comes from the same successful presentation.
    pub fn geometry(&self) -> &TextGeometry {
        &self.geometry
    }
}
