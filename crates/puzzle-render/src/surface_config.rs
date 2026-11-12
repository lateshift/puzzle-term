//! D-27 uses encoded sRGB components and gamma-space blending.
//! An sRGB attachment would encode those bytes a second time. Attachment format
//! and compositor colour space are independent: Unorm preserves the bytes, while
//! sRGB metadata tells wide-gamut displays how those bytes must be interpreted.
//! Both native host contracts and the browser use this same configuration path.
use crate::{RenderError, Renderer};

/// Host-independent policy keeps surface setup and shader attachment formats identical.
impl Renderer {
    /// Builds the configuration used by both surface creation and immutable pipelines.
    pub(crate) fn surface_configuration(
        &self,
        surface: &wgpu::Surface<'_>,
        width: u32,
        height: u32,
    ) -> Result<wgpu::SurfaceConfiguration, RenderError> {
        // Keep the platform's other defaults, including alpha mode and view formats.
        let mut config = surface
            .get_default_config(&self.adapter, width.max(1), height.max(1))
            .ok_or(RenderError::NoSurfaceConfiguration)?;
        // Defaults are preferences, not proof that their non-sRGB sibling is
        // supported. Confirm the actual format before wgpu can validate it.
        let capabilities = surface.get_capabilities(&self.adapter);
        config.format = encoded_format(config.format, &capabilities.formats)?;
        // Request sRGB presentation and disable wide-gamut/EDR configuration.
        // Metal maps this to nil; the native embed seam reapplies named sRGB
        // after configure so Core Animation performs explicit display matching.
        config.color_space = wgpu::SurfaceColorSpace::Srgb;
        config.present_mode = wgpu::PresentMode::Fifo;
        // D-35's queue depth remains independent of the colour correction.
        config.desired_maximum_frame_latency = self.maximum_frame_latency();
        if self.surface_readback {
            // A real-drawable witness must fail if copies are unsupported, not fake
            // evidence from an offscreen texture. Product surfaces stay write-only.
            if !capabilities.usages.contains(wgpu::TextureUsages::COPY_SRC) {
                return Err(RenderError::Readback(
                    "surface cannot copy actual drawable".into(),
                ));
            }
            config.usage |= wgpu::TextureUsages::COPY_SRC;
        }
        Ok(config)
    }
}

/// Preserves channel order/preference while rejecting a hardware encoding step.
fn encoded_format(
    preferred: wgpu::TextureFormat,
    supported: &[wgpu::TextureFormat],
) -> Result<wgpu::TextureFormat, RenderError> {
    // Removing the suffix changes hardware conversion, not channel order or
    // precision. This keeps packed colours and shader output byte-identical.
    let encoded = preferred.remove_srgb_suffix();
    if supported.contains(&encoded) {
        return Ok(encoded);
    }
    // An adapter may advertise another compatible channel order instead.
    supported
        .iter()
        .copied()
        .find(|format| !format.is_srgb())
        .ok_or(RenderError::NoSurfaceConfiguration)
}

#[cfg(test)]
/// Hardware encoding must never be silently selected as a compatibility fallback.
mod tests;
