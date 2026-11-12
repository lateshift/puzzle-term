// Test-only Slug fragment input probe (design §9.6).
//
// The reference shader interpolates em-space texcoord across its dilated
// quad. Metal and browser rasterizers may round that coordinate differently
// from an exact inverse affine transform at the pixel centre. This entry
// point records the actual inputs consumed by SlugRender, allowing the CPU
// oracle to assess its math without that interpolation discrepancy.
// It is appended to the production source only under `oracle-debug`.

// RGBA32Float stores em x, em y, fwidth x and fwidth y without quantizing
// the inputs to eight-bit colors before the reference function sees them.
// A second integer attachment identifies which atlas instance produced the
// captured inputs. The test-only instance paint word carries index + 1, so
// zero still means no fragment after the clear. A third float attachment
// preserves that fragment's coverage before neighboring quads blend into it.
// Geometry and the reference coverage function are unchanged.
struct DebugOutput {
    @location(0) coordinates: vec4<f32>,
    @location(1) instance_index: u32,
    @location(2) coverage: f32,
};

@fragment
fn fs_debug_inputs(input: VertexOutput) -> DebugOutput {
    let ems_per_pixel = fwidth(input.texcoord);
    let coverage = SlugRender(input.texcoord, input.banding, input.glyph);
    return DebugOutput(vec4<f32>(input.texcoord, ems_per_pixel), input.paint, coverage);
}
