//! A frame that cannot admit any cold ink must still present a provisional box.
use super::*;
use puzzle_term::text::preparation::{Completion, Driver, Ink, Job};
use std::time::Duration;

/// Both the actual glyph and diagnostic vector preparation are deliberately declined.
#[wasm_bindgen_test(async)]
async fn declined_cold_glyph_presents_box_instead_of_empty_cell() {
    let (terminal, host) = presentation::empty_terminal().await;
    let resource = terminal.owner.borrow().text.clone().unwrap();
    JsFuture::from(super::super::resources::load_fonts(&resource))
        .await
        .unwrap();
    resource
        .context
        .borrow_mut()
        .as_mut()
        .unwrap()
        .set_preparation_driver(Box::new(Decline))
        .unwrap();
    // Ω is covered by the full font, absent from the startup pack, and still cold.
    JsFuture::from(terminal.write("Ω".into()).unwrap())
        .await
        .unwrap();
    let pixels = presentation::presented_pixels(&terminal).await;
    let result = presentation::assert_cells(&terminal, &pixels, &["Ω"], 1, 0);
    terminal.dispose().unwrap();
    host.remove();
    assert!(result.is_ok(), "{}", result.unwrap_err());
}

/// No worker or inline preparation can accidentally satisfy the provisional witness.
struct Decline;
/// A scheduler with no admission allowance and no accepted jobs.
impl Driver for Decline {
    /// Every frame remains unable to prepare cold geometry.
    fn begin_frame(&mut self, _: Duration) {}
    /// Refuses real font and procedural diagnostic jobs alike.
    fn prepare_now(&mut self, _: &Job) -> Option<Result<Option<Ink>, String>> {
        None
    }
    /// Leaves refused keys retryable rather than manufacturing an empty glyph fact.
    fn submit(&mut self, _: Job) -> bool {
        false
    }
    /// No completion can replace the border before pixels are captured.
    fn poll(&mut self) -> Option<Completion> {
        None
    }
    /// Outstanding refused work never wakes the idle screen.
    fn ready(&mut self) -> bool {
        false
    }
    /// No job is owned by this scheduler.
    fn pending(&self) -> bool {
        false
    }
    /// Refused work costs no elastic preparation time.
    fn frame_time(&self) -> Duration {
        Duration::ZERO
    }
}
