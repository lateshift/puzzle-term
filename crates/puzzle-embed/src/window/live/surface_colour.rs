//! Native colour witness copies the actual drawable before presenting it.
//! Records are written only after that drawable has entered the present queue.
//! First-frame padding exposes the theme clear without glyph coverage or overlays.
//! No screenshot oracle, display profile conversion or expected-value shader is used.
use super::LiveWindow;
use std::io::Write;

/// Observation belongs to the render owner, which alone holds the acquired texture.
impl LiveWindow {
    /// Collects optional first-frame evidence; errors remain explicit failed records.
    pub(super) fn capture_surface_colour(
        &self,
        texture: &wgpu::Texture,
    ) -> Option<serde_json::Value> {
        // Startup is enough to expose double encoding in the theme background.
        // Do not repeatedly stall the queue while an interactive shell is running.
        if self.next_frame != 0 || !self.renderer.surface_readback_enabled() {
            return None;
        }
        // The upper-left padding pixel is the theme's premultiplied clear colour.
        let expected: Vec<u8> = self
            .frame
            .clear
            .iter()
            .map(|v| (v * 255.0).round() as u8)
            .collect();
        // Inspect configured metadata as well as the actual texture format: the two
        // contracts are independent, particularly for a borrowed wide-gamut layer.
        let config = self.surface.get_configuration();
        // The record keeps dimensions to distinguish this native drawable from the
        // small offscreen oracle targets used elsewhere in screenshot verification.
        let mut record = serde_json::json!({
            "format": format!("{:?}", texture.format()),
            "srgb_format": texture.format().is_srgb(),
            "color_space": config.map(|c| format!("{:?}", c.color_space)),
            "expected": expected,
            "extent": [texture.width(), texture.height()],
        });
        #[cfg(target_os = "macos")]
        {
            // The requested enum alone cannot detect wgpu clearing native metadata.
            record["layer_srgb"] = serde_json::json!(
                puzzle_sys::layer::colour::surface_is_srgb(&self.surface).unwrap_or(false)
            );
        }
        // Errors are retained in the receipt; missing bytes can never equal expected.
        // A readback does not redraw the frame or apply a CPU colour conversion.
        match self.renderer.read_surface_texture(texture) {
            Ok(image) => record["actual"] = serde_json::json!(&image.pixels[..4]),
            Err(error) => record["error"] = serde_json::json!(error.to_string()),
        }
        // Return owned evidence without retaining the drawable beyond presentation.
        Some(record)
    }

    /// Persists only evidence for the texture that was actually submitted to present.
    pub(super) fn record_surface_colour(&self, record: Option<serde_json::Value>) {
        // Nothing is formatted or written on the ordinary product path.
        let Some(record) = record else {
            return;
        };
        // The path is an explicit opt-in developer receipt, never terminal input.
        let Some(path) = std::env::var_os("PUZZLE_SURFACE_COLOUR_REPORT") else {
            return;
        };
        // One render owner serializes records across every surface in the process.
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| writeln!(file, "{record}"));
        // A missing/unwritable receipt makes the acceptance reader fail; also keep
        // the original I/O reason in tracing for a developer running the probe alone.
        if let Err(error) = written {
            tracing::error!(event="desktop.surface_colour_report_failed", %error);
        }
        tracing::info!(event="desktop.surface_colour", evidence=%record);
    }
}
