//! Pixel and cell occupancy assertions for headless GPU evidence.
//!
//! Separating these from scenario dispatch keeps the command grammar small
//! while allowing each assertion to explain its own coordinate system.
//! Pixel assertions are intentionally independent of terminal geometry:
//! golden scenes often place glyphs at arbitrary physical coordinates.
//! Cell assertions instead require a completed terminal render, because
//! font metrics and the fixed grid inset are known only after frame build.
//! The assertion path never predicts coverage from source bytes. A passing
//! result must come from GPU pixels returned by the readback path.

use super::{Scenario, ink};

impl Scenario {
    /// Verifies an RGBA pixel with the scenario's per-channel tolerance.
    ///
    /// Pixel coordinates are device pixels. The assertion compares red,
    /// green, and blue independently so one wrong channel cannot be hidden
    /// by another channel's opposing error.
    pub(super) fn expect_pixel<'a>(
        &self,
        parts: &mut impl Iterator<Item = &'a str>,
    ) -> Result<(), String> {
        // The script grammar uses unsigned device coordinates. Negative
        // values cannot address readback storage and are rejected here.
        let x: u32 = parts
            .next()
            .ok_or("missing x")?
            .parse()
            .map_err(|_| "invalid x")?;
        // Preserve a separate error for y so a typo names the bad field.
        let y: u32 = parts
            .next()
            .ok_or("missing y")?
            .parse()
            .map_err(|_| "invalid y")?;
        // Parse the intended RGB value before touching the image. A
        // malformed assertion should fail even if no frame was produced.
        let color = parts.next().ok_or("missing color")?.trim_start_matches('#');
        let expected = u32::from_str_radix(color, 16).map_err(|_| "invalid color")?;
        // Six digits avoid accidentally accepting an alpha-bearing color
        // or a shorthand whose interpretation would be ambiguous.
        if color.len() != 6 {
            return Err("color must be #RRGGBB".into());
        }
        // Different GPU paths may round a channel by one byte. The script
        // supplies that permitted error explicitly for each assertion.
        let tolerance: u8 = parts
            .next()
            .ok_or("missing tolerance")?
            .parse()
            .map_err(|_| "invalid tolerance")?;
        // Readback is the only evidence here. No CPU-side scene color is
        // substituted when the caller forgot to render a screenshot.
        let image = self
            .image
            .as_ref()
            .ok_or("expect-pixel requires a preceding screenshot")?;
        // The image may be a crop smaller than the M0 placeholder grid;
        // never clamp an out-of-bounds expected pixel into another one.
        if x >= image.width || y >= image.height {
            return Err("pixel outside image".into());
        }
        // Four bytes per pixel include alpha, but the M0 command names RGB.
        // Multiplication uses the packed stride established by RgbaImage,
        // rather than wgpu's padded transfer-row stride.
        let offset = ((y * image.width + x) * 4) as usize;
        // Convert the script's hexadecimal order into the exact three
        // channel bytes yielded by the RGBA readback.
        let wanted = [
            (expected >> 16) as u8,
            (expected >> 8) as u8,
            expected as u8,
        ];
        // Compare channels separately: opposing errors in red and blue
        // cannot cancel as they would in a luminance-only comparison.
        for (actual, want) in image.pixels[offset..offset + 3].iter().zip(wanted) {
            // A small tolerance accounts for backend conversion differences.
            if actual.abs_diff(want) > tolerance {
                return Err(format!(
                    "pixel ({x},{y}) was {:02x?}, wanted #{color} ±{tolerance}",
                    &image.pixels[offset..offset + 3]
                ));
            }
        }
        Ok(())
    }

    /// Rejects blank or overfilled regions using actual GPU readback bytes.
    pub(super) fn expect_ink<'a>(
        &self,
        parts: &mut impl Iterator<Item = &'a str>,
    ) -> Result<(), String> {
        // A terminal assertion needs the image from render-terminal, not
        // an older freeform specimen kept in the scenario's image slot.
        let image = self
            .image
            .as_ref()
            .ok_or("expect-ink requires a preceding screenshot")?;
        ink::check(image, parts, Some(self.cell_geometry()?))
    }

    /// Keeps freeform glyph scenes in device pixels without inventing cells.
    pub(super) fn expect_ink_pixels<'a>(
        &self,
        parts: &mut impl Iterator<Item = &'a str>,
    ) -> Result<(), String> {
        // Synthetic Slug shapes and M4 specimens have no terminal rows;
        // their explicit device-pixel rectangle is checked by ink::check.
        let image = self
            .image
            .as_ref()
            .ok_or("expect-ink-px requires a preceding screenshot")?;
        ink::check(image, parts, None)
    }

    /// Compares equal-width cell slices after removing their row offset.
    pub(super) fn expect_cells_identical<'a>(
        &self,
        parts: &mut impl Iterator<Item = &'a str>,
    ) -> Result<(), String> {
        let image = self
            .image
            .as_ref()
            .ok_or("expect-cells-identical requires a screenshot")?;
        // The two samples come from the same readback and therefore share
        // font hinting, antialiasing and theme colors exactly.
        let first = parts.next().ok_or("missing first cell range")?;
        let second = parts.next().ok_or("missing second cell range")?;
        ink::identical::check(image, self.cell_geometry()?, first, second)
    }

    /// Returns only geometry measured by the currently rendered VT frame.
    fn cell_geometry(&self) -> Result<ink::CellGeometry, String> {
        if !self.preview_ready {
            return Err("cell assertion requires a rendered terminal grid".into());
        }
        // The current VT size replaces any earlier placeholder `size`.
        let (cols, rows) = self
            .terminal
            .as_ref()
            .ok_or("terminal grid missing")?
            .size();
        // The frame builder used these physical dimensions in the image.
        let metrics = self
            .frame_builder
            .as_ref()
            .ok_or("terminal metrics missing")?
            .metrics();
        Ok(ink::CellGeometry {
            cols: u32::try_from(cols).map_err(|_| "terminal columns overflow")?,
            rows: u32::try_from(rows).map_err(|_| "terminal rows overflow")?,
            width: metrics.width_px,
            height: metrics.height_px,
        })
    }
}
