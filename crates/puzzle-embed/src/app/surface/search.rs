//! Native find components use queued owner messages; there is no Session getter lock.
use super::Surface;
use crate::{ApiError, SearchUi};
use puzzle_term::SessionMessage;

impl Surface {
    /// Choose built-in or host controls; later keys observe this in FIFO order.
    pub fn set_search_ui(&self, ui: SearchUi) -> Result<(), ApiError> {
        self.send(SessionMessage::SearchUi(ui))
    }
    /// Request plain selection text, returned as HostEvent::SelectedText on App::tick.
    /// The token is returned unchanged, letting hosts ignore outdated requests.
    /// Empty selection returns an empty string; no clipboard side effect occurs.
    pub fn selected_text(&self, request: u64) -> Result<(), ApiError> {
        self.send(SessionMessage::SelectedText(request))
    }
}
