//! Small deterministic easing functions shared by providers (design §11.5).
//!
//! No helper reads a wall clock or starts a timer. Providers receive time
//! from Session and return deadlines through Requests, so idle remains free.
/// Clamps interpolation to endpoints, treating invalid clocks as completion.
pub fn fraction(now: f64, start: f64, end: f64, reduced_motion: bool) -> f32 {
    if reduced_motion || end <= start || !now.is_finite() {
        return 1.0;
    }
    ((now - start) / (end - start)).clamp(0.0, 1.0) as f32
}
/// Cubic ease-out for a fade or a short motion toward its final state.
pub fn ease_out(t: f32) -> f32 {
    let remaining = 1.0 - t.clamp(0.0, 1.0);
    1.0 - remaining * remaining * remaining
}
/// Smooth endpoint velocity for a transition that begins and ends at rest.
pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
