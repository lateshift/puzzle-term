//! Deterministic M3 Slug scenes shared by the native and browser shells.
//!
//! Every scene draws white outlines on opaque black. Single-outline cases
//! expose a raw coverage byte; specimen lines also composite overlaps in
//! draw order. The font scene receives approved JetBrains Mono bytes from
//! its platform, while synthetic scenes isolate scale and affine expansion.
//! This small composition helper
//! belongs above geometry and fonts, and contains no GPU or platform APIs.
//!
//! A scene stores the packed handle as well as its instance. The GPU uses
//! the instance, and the oracle uses the same atlas handle and inverse
//! affine map. This prevents a comparison from silently preparing a second
//! copy of the outline with different band or packing choices.
//!
//! The original seven fixtures isolate geometry and a single capital B.
//! The full specimen matrix adds a repeated real-font line at nine logical
//! sizes and two device pixel ratios. Its affine variants stress the vertex
//! inverse Jacobian and dynamic dilation using the same packed outlines.
//! Names are stable because scenario scripts, GPU diagnostic logs and PNG
//! references all identify a failing configuration with the same token.
//!
//! Instances stay in draw order beside matching atlas handles. Repeated
//! characters share curve and band texels, but their integer origins differ.
//! The CPU oracle needs the parallel handle list to choose the correct
//! winding data for each draw, while the GPU consumes only packed instance
//! fields. The platform provides font bytes before either is constructed.
use puzzle_frame::slug::SlugInstance;
use puzzle_slug::{
    ShapeKey, SlugAtlas,
    atlas::{FillRule, ShapeHandle},
    synthetic::SyntheticShape,
};
pub use puzzle_text::FontData;
use puzzle_text::probe::ProbeFont;
/// Oracle compositing and exact-affine inverse sampling for all instances.
mod oracle;
/// Real-font specimen lines with affine transforms and whole-pixel origins.
mod specimen;

/// Affine treatment applied to an entire real-font specimen line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecimenTreatment {
    /// Ordinary horizontal baseline at the requested device size.
    Normal,
    /// Fifteen-degree clockwise rotation around the line origin.
    Rotate15,
    /// Quarter-turn rotation checks axis exchange in the Jacobian.
    Rotate90,
    /// Twelve-degree rightward synthetic italic shear.
    Shear12,
    /// Fourfold enlargement reuses the same prepared em outlines.
    Zoom4,
}

/// Named fixtures of the M3 geometry and font coverage suite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlugSceneKind {
    /// Small synthetic shape at one device pixel per logical pixel.
    Small,
    /// Large synthetic outline at two device pixels per logical pixel.
    Large,
    /// A real capital B at terminal size and DPR one.
    Glyph,
    /// A real capital B at a larger size and DPR two.
    GlyphLarge,
    /// A rotated curved outline checks both Jacobian components.
    Rotated,
    /// A sheared curved outline checks nonorthogonal derivatives.
    Sheared,
    /// A zoomed outline checks that em-space atlas data is size independent.
    Zoomed,
    /// The full JetBrains Mono probe line at a size, DPR and transform.
    Specimen {
        /// Logical font size in pixels before DPR and optional zoom.
        size_px: u16,
        /// Device pixels per logical pixel, one or two in M3 fixtures.
        dpr: u8,
        /// Whole-line affine treatment after size scaling.
        treatment: SpecimenTreatment,
    },
}

impl SlugSceneKind {
    /// Gives artifacts stable filenames matching their scenario names.
    pub fn name(self) -> String {
        match self {
            Self::Small => "small".into(),
            Self::Large => "large".into(),
            Self::Glyph => "glyph".into(),
            Self::GlyphLarge => "glyph-large".into(),
            Self::Rotated => "rotated".into(),
            Self::Sheared => "sheared".into(),
            Self::Zoomed => "zoomed".into(),
            Self::Specimen {
                size_px,
                dpr,
                treatment,
            } => {
                let suffix = match treatment {
                    SpecimenTreatment::Normal => "",
                    SpecimenTreatment::Rotate15 => "rot15-",
                    SpecimenTreatment::Rotate90 => "rot90-",
                    SpecimenTreatment::Shear12 => "shear12-",
                    SpecimenTreatment::Zoom4 => "zoom4-",
                };
                format!("specimen-{suffix}{size_px}-{dpr}x")
            }
        }
    }

    /// Parses names used in scripts and the browser's scene query.
    pub fn parse(value: &str) -> Option<Self> {
        let fixed = match value {
            "small" => Some(Self::Small),
            "large" => Some(Self::Large),
            "glyph" => Some(Self::Glyph),
            "glyph-large" => Some(Self::GlyphLarge),
            "rotated" => Some(Self::Rotated),
            "sheared" => Some(Self::Sheared),
            "zoomed" => Some(Self::Zoomed),
            _ => None,
        };
        if fixed.is_some() {
            return fixed;
        }
        specimen::parse_name(value)
    }

    /// Lists every fixture so acceptance cannot accidentally skip one.
    pub fn all() -> Vec<Self> {
        // The fixed cases catch packing and Jacobian errors without the
        // font loader; the remaining cases test real outlines at scale.
        let mut cases = vec![
            Self::Small,
            Self::Large,
            Self::Glyph,
            Self::GlyphLarge,
            Self::Rotated,
            Self::Sheared,
            Self::Zoomed,
        ];
        // Nine logical sizes at two DPRs cover the M3 quality range. Four
        // transforms run at a representative 24 px at both DPRs.
        for size_px in [6, 8, 10, 12, 14, 16, 24, 48, 96] {
            for dpr in [1, 2] {
                cases.push(Self::Specimen {
                    size_px,
                    dpr,
                    treatment: SpecimenTreatment::Normal,
                });
            }
        }
        for treatment in [
            SpecimenTreatment::Rotate15,
            SpecimenTreatment::Rotate90,
            SpecimenTreatment::Shear12,
            SpecimenTreatment::Zoom4,
        ] {
            for dpr in [1, 2] {
                cases.push(Self::Specimen {
                    size_px: 24,
                    dpr,
                    treatment,
                });
            }
        }
        cases
    }
}

/// One outline, its packed texture data and the draw record for one image.
pub struct SlugScene {
    /// CPU mirror whose dirty spans are uploaded by the renderer.
    pub atlas: SlugAtlas,
    /// White instances sharing the same packed atlas and target.
    pub instances: Vec<SlugInstance>,
    /// Device-pixel width of both GPU and oracle targets.
    pub width: u32,
    /// Device-pixel height of both GPU and oracle targets.
    pub height: u32,
    /// Parallel handles retained for CPU sampling after GPU uploads.
    handles: Vec<ShapeHandle>,
}

impl SlugScene {
    /// Prepares a named geometry or real-font outline once in em space.
    ///
    /// The platform provides font bytes only when the kind requires them;
    /// neutral composition code never embeds or loads an asset itself.
    pub fn new(kind: SlugSceneKind, font_data: Option<&FontData>) -> Result<Self, String> {
        // The line builder owns many instances and computes its own union
        // bounds; a single-glyph fixture keeps the established small crop.
        if let SlugSceneKind::Specimen {
            size_px,
            dpr,
            treatment,
        } = kind
        {
            return specimen::build(size_px, dpr, treatment, font_data);
        }
        let path = match kind {
            SlugSceneKind::Glyph | SlugSceneKind::GlyphLarge => {
                // The platform fetched or read the same licensed file. A
                // missing face is reported rather than replaced by a shape.
                let data = font_data.ok_or("Slug glyph scene needs font bytes")?;
                if data.index() != 0 {
                    return Err("M3 outline probe supports face index zero only".into());
                }
                let font = ProbeFont::new(data.bytes()).map_err(|error| error.to_string())?;
                font.glyph('B').map_err(|error| error.to_string())?.0
            }
            SlugSceneKind::Small | SlugSceneKind::Large => SyntheticShape::Rectangle.path(),
            SlugSceneKind::Rotated | SlugSceneKind::Sheared | SlugSceneKind::Zoomed => {
                SyntheticShape::TypicalGlyph.path()
            }
            SlugSceneKind::Specimen { .. } => return Err("invalid specimen dispatch".into()),
        };
        // One shape per atlas makes the fixture's band origin stable even
        // when unrelated font glyphs are prepared elsewhere in a session.
        let mut atlas = SlugAtlas::default();
        let handle = atlas
            .prepare(ShapeKey(1), &path, FillRule::NonZero, 0)
            .map_err(|error| error.to_string())?;
        let (m, t, width, height) = transform(kind).ok_or("specimen needs its line builder")?;
        // m maps em coordinates to device pixels. The same transform is
        // inverted below for CPU sampling, while the GPU expands its bbox.
        // Every source is solid white: premultiplied output over black
        // exposes a raw Slug coverage byte for strict oracle comparison.
        let instance = SlugInstance {
            m,
            t,
            bbox: handle.bbox,
            glyph_loc: puzzle_slug::encoding::encode_glyph_loc(handle.glyph_loc),
            band_info: handle.band_info,
            color: 0xffff_ffff,
            paint: 0,
        };
        Ok(Self {
            atlas,
            instances: vec![instance],
            width,
            height,
            handles: vec![handle],
        })
    }
}

/// Supplies em-to-device matrices with nonoverlapping image margins.
fn transform(kind: SlugSceneKind) -> Option<([f32; 4], [f32; 2], u32, u32)> {
    Some(match kind {
        // The 6 px and 96 px endpoints catch precision errors at both
        // ends of the specified M3 pixel-size range.
        SlugSceneKind::Small => ([6.0, 0.0, 0.0, -6.0], [17.0, 27.0], 44, 44),
        SlugSceneKind::Large => ([192.0, 0.0, 0.0, -192.0], [28.0, 220.0], 248, 248),
        SlugSceneKind::Glyph => ([11.0, 0.0, 0.0, -11.0], [18.0, 28.0], 48, 48),
        SlugSceneKind::GlyphLarge => ([192.0, 0.0, 0.0, -192.0], [24.0, 220.0], 248, 248),
        // Rotation, shear and zoom are applied in the instance rather
        // than to atlas texels, preserving the size-independent store.
        SlugSceneKind::Rotated => ([27.0, 13.0, 13.0, -27.0], [30.0, 57.0], 88, 88),
        SlugSceneKind::Sheared => ([48.0, 0.0, 16.0, -48.0], [20.0, 76.0], 112, 112),
        SlugSceneKind::Zoomed => ([64.0, 0.0, 0.0, -64.0], [22.0, 88.0], 128, 128),
        SlugSceneKind::Specimen { .. } => return None,
    })
}
