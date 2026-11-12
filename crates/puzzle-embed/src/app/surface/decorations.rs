//! Native annotations use the same bounded Session controller as web and harness.
//!
//! Source markers are created on the Session owner; hosts receive their result
//! through App::tick. No host must block for a marker or lend a live VT borrow.
//! Correlated additions answer through the ordinary host event queue, after the
//! Session owner has checked the requested cell or declaration. No tracked
//! position or mutable registry is borrowed across the input endpoint. Updates
//! and removals preserve channel order with later pointer and frame requests.
use super::*;
use puzzle_term::overlay::{
    anchor::Marker,
    decoration::{Decoration, DecorationId},
    geometry::Cell,
};

impl Surface {
    /// Creates a tracked source endpoint; HostEvent::MarkerAdded correlates the result.
    pub fn add_marker(&self, request: u64, cell: Cell) -> Result<(), ApiError> {
        self.send(SessionMessage::AddMarker { request, cell })
    }
    /// Releases caller ownership; declarations retaining this marker become Gone.
    pub fn remove_marker(&self, marker: Marker) -> Result<(), ApiError> {
        self.send(SessionMessage::RemoveMarker(marker))
    }
    /// Queues a declaration; HostEvent::DecorationAdded carries its validated identity.
    pub fn add_decoration(&self, request: u64, declaration: Decoration) -> Result<(), ApiError> {
        self.send(SessionMessage::AddDecoration {
            request,
            declaration,
        })
    }
    /// Replaces a declaration atomically on the owner; rejection preserves the old one.
    pub fn update_decoration(
        &self,
        id: DecorationId,
        declaration: Decoration,
    ) -> Result<(), ApiError> {
        self.send(SessionMessage::UpdateDecoration(id, declaration))
    }
    /// Disposes an annotation without releasing any caller-owned source marker.
    pub fn remove_decoration(&self, id: DecorationId) -> Result<(), ApiError> {
        self.send(SessionMessage::RemoveDecoration(id))
    }
}
