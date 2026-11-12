//! One CPU-oriented Canvas belongs to a shared text resource, not a global lane.
//!
//! Drivers carry its existing wake identity as Rust text. Temporary lookup uses
//! the sanctioned weak text registry; it never puts JS handles into Send core state.
//! Disposal drops the canvas with the text resource and cannot recreate an owner.
//!
//! Emoji and text fallback share this surface because their jobs run serially.
//! Each draw owns its readback bytes before another job can clear the stage.
//! Width/height are limited by the raster-page contract at the source boundary.
//! Resizing changes backing dimensions while preserving the context's identity.
//! Equal dimensions avoid resizing but still require clearing previous ink.
//! Transform reset prevents one fitted glyph from shifting the following one.
//! White fill restores mask extraction after a prior color/style operation.
//! CSS font and glyph fitting are supplied anew by the source on every draw.
//! Registry lookup releases its borrow before accessing the resource's surface.
//! The weak registry contains only its already-authorized GPU/text owners.
//! No extra page-global canvas or core-side JavaScript object is introduced.
//! Lazy allocation keeps bundled startup independent of browser font fallback.
use super::*;
use web_sys::OffscreenCanvasRenderingContext2d;

/// A text resource retains one context across sizes, scripts and both source policies.
#[derive(Default)]
pub(super) struct Surface {
    /// Lazy creation avoids any Canvas allocation on ordinary bundled-text startup.
    context: Option<OffscreenCanvasRenderingContext2d>,
}
/// The platform owner can retain browser handles without changing the core Send seam.
impl Surface {
    /// Resets transform and samples while preserving the readback-oriented context.
    pub(super) fn get(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<(OffscreenCanvasRenderingContext2d, bool), String> {
        let created = self.context.is_none();
        if created {
            self.context = Some(crate::runtime::fallback::context(width, height)?);
        }
        let context = self.context.as_ref().ok_or("readback context absent")?;
        let canvas = context.canvas();
        // Resize only when necessary; setting a size also resets Canvas state.
        if canvas.width() != width {
            canvas.set_width(width);
        }
        if canvas.height() != height {
            canvas.set_height(height);
        }
        context
            .reset_transform()
            .map_err(crate::runtime::fallback::js_error)?;
        context.clear_rect(0.0, 0.0, f64::from(width), f64::from(height));
        context.set_fill_style_str("#ffffff");
        Ok((context.clone(), created))
    }
}

/// Resolve only an existing shared resource; standalone source tests use a local surface.
pub(in crate::runtime) fn context(
    name: &str,
    width: u32,
    height: u32,
) -> Result<Option<(OffscreenCanvasRenderingContext2d, bool)>, String> {
    let owner = REGISTRY.with(|registry| {
        registry
            .borrow()
            .text
            .values()
            .filter_map(Weak::upgrade)
            .find(|text| {
                text.fallback
                    .borrow()
                    .as_ref()
                    .is_some_and(|wake| wake.name == name)
            })
    });
    owner
        .map(|text| text.readback.borrow_mut().get(width, height))
        .transpose()
}

/// Pixel comparisons protect reuse across geometry and transform changes.
#[cfg(test)]
mod tests;
