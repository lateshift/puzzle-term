//! Main-thread colour matching for uncovered bounds; no retained-image scaling.
use super::*;
use objc2_core_graphics::CGColor;

impl PresentationLayer {
    /// Native colours are explicitly sRGB, never device-space/P3 reinterpretations.
    pub(super) fn apply_background(&self) {
        // Copy numeric state before any native property access; no Objective-C
        // call or allocation may hold the render/completion mailbox guard.
        let colour = self.ivars().shared.lock().background;
        // Ordinary frames reuse the retained CGColor without allocating.
        if colour == self.ivars().background.get() {
            return;
        }
        let Some(rgba) = colour else { return };
        // The renderer produces finite normalized components. Guard this safe
        // boundary too: malformed host-side values must not reach Core Graphics.
        if rgba
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            tracing::warn!(event = "native.invalid_background");
            return;
        }
        let [r, g, b, a] = rgba.map(f64::from);
        let colour = CGColor::new_srgb(r, g, b, a);
        // CALayer retains the colour. The source pixel values and alpha are
        // unchanged; only the uncovered part of this child needs the backing.
        self.setBackgroundColor(Some(&colour));
        self.ivars().background.set(Some(rgba));
    }
}
