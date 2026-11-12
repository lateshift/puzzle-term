// Atlas-backed grid and transformed bitmap-only ink (design §8.9, §10.3).
//
// Ordinary grid quads retain texel-exact nearest sampling. Effect quads
// fetch their forward linear map and tint from the shared paint table,
// preserving RasterInstance's forty-byte layout. Four straight-alpha
// samples are premultiplied BEFORE interpolation so transparent RGB
// cannot create a dark or colored fringe along a rotated glyph edge.
// The CPU includes one transparent atlas-border texel around effect quads.

// Device-pixel projection, with explicit padding matching its uniform.
struct Viewport {
    // Physical attachment extent, never CSS dimensions.
    size: vec2<f32>,
    // Uniform alignment padding carries no terminal geometry.
    padding: vec2<f32>,
}
// The vertex stage projects physical positions into WebGPU clip space.
@group(0) @binding(0) var<uniform> viewport: Viewport;
// Coverage-only samples use one channel, retaining the instance foreground.
@group(0) @binding(1) var mask_atlas: texture_2d_array<f32>;
// Authored RGBA remains straight alpha in the atlas's CPU/GPU mirrors.
@group(0) @binding(2) var color_atlas: texture_2d_array<f32>;
// Nearest sampling is unchanged for ordinary pixel-aligned grid text.
@group(0) @binding(3) var atlas_sampler: sampler;
// Existing 256-texel paint rows also carry bitmap effect matrices and tints.
@group(0) @binding(4) var effect_table: texture_2d<f32>;

// Attributes mirror the pinned forty-byte RasterInstance contract.
struct RasterInput {
    // Transformed origin plus untransformed width/height for effects; plain rect otherwise.
    @location(0) rect: vec4<f32>,
    // Normalized crop edges, including transparent border for effects.
    @location(1) uv: vec4<f32>,
    // Ordinary RGBA8, or the paint-table index when flag bit one is set.
    @location(2) tint: u32,
    // Bit zero: color; bit one: effect; bit two: replace color; bit three: filtered image; bits 8–15: page; bits 16–31: ordered incarnation.
    @location(3) flags: u32,
}
// UV interpolation follows the affine quad without changing glyph placement.
struct RasterOutput {
    // Physical geometry projected into WebGPU clip space.
    @builtin(position) clip: vec4<f32>,
    // Affine atlas coordinate at this fragment.
    @location(0) uv: vec2<f32>,
    // Premultiplied tint/opacity stays constant across both triangles.
    @location(1) @interpolate(flat) tint: vec4<f32>,
    // Sampling and palette policies must not interpolate between vertices.
    @location(2) @interpolate(flat) flags: u32,
    // Images retain one device-space transform across independently clipped cells.
    @location(3) @interpolate(flat) rect: vec4<f32>,
    // Full-placement UV edges avoid independently rounded per-cell source crops.
    @location(4) @interpolate(flat) crop: vec4<f32>,
}
// Converts straight RGBA8 to the shared premultiplied convention once per vertex.
fn tint_color(packed: u32) -> vec4<f32> {
    let rgba = vec4<f32>(f32(packed & 255u), f32((packed >> 8u) & 255u),
        f32((packed >> 16u) & 255u), f32(packed >> 24u)) / 255.0;
    // Coverage later multiplies every channel by the same fraction.
    return vec4<f32>(rgba.rgb * rgba.a, rgba.a);
}
// Expands one retained record into six vertices without CPU-side tessellation.
@vertex fn raster_vertex(@builtin(vertex_index) vertex: u32, input: RasterInput) -> RasterOutput {
    // Fixed triangle-list corners are shared by plain and affine bitmap quads.
    let corners = array<vec2<f32>, 6>(vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0));
    let corner = corners[vertex];
    var offset = corner * input.rect.zw;
    var tint = tint_color(input.tint);
    if (input.flags & 2u) != 0u {
        // Index is linear first-texel-plus-one, matching the Slug paint convention.
        let row = i32((input.tint - 1u) / 256u);
        let m = textureLoad(effect_table, vec2<i32>(1, row), 0);
        // Column-major forward matrix acts only on the quad's relative offset.
        offset = vec2<f32>(m.x * offset.x + m.z * offset.y, m.y * offset.x + m.w * offset.y);
        tint = textureLoad(effect_table, vec2<i32>(5, row), 0);
    }
    let device = input.rect.xy + offset;
    var output: RasterOutput;
    // Destination coordinates already include the transformed origin and translation.
    output.clip = vec4<f32>(device.x * 2.0 / viewport.size.x - 1.0,
        1.0 - device.y * 2.0 / viewport.size.y, 0.0, 1.0);
    output.uv = mix(input.uv.xy, input.uv.zw, corner);
    output.tint = tint;
    output.flags = input.flags;
    output.rect = input.rect;
    output.crop = input.uv;
    return output;
}
// Fetches one texel in premultiplied coordinates, before any filtering.
fn sample_texel(at: vec2<i32>, flags: u32) -> vec4<f32> {
    // Clamping only protects the texture boundary; the crop's border supplies transparency.
    // Image textures share the shader but have their own source dimensions.
    let extent = select(textureDimensions(mask_atlas), textureDimensions(color_atlas), (flags & 1u) != 0u);
    let p = clamp(at, vec2<i32>(0), vec2<i32>(extent) - vec2<i32>(1));
    let page = i32((flags >> 8u) & 255u);
    if (flags & 1u) != 0u {
        let color = textureLoad(color_atlas, p, page, 0);
        // Ignore undefined straight RGB in a transparent texel before interpolation.
        return vec4<f32>(color.rgb * color.a, color.a);
    }
    // White premultiplied coverage can later take any foreground tint.
    let coverage = textureLoad(mask_atlas, p, page, 0).r;
    return vec4<f32>(coverage);
}
// Four taps reproduce bilinear sampling in the alpha convention required by §8.9.
fn filtered(input: RasterOutput) -> vec4<f32> {
    // Normalized UV centers map to integer texel indices after the half-texel shift.
    let extent = select(textureDimensions(mask_atlas), textureDimensions(color_atlas), (input.flags & 1u) != 0u);
    var uv = input.uv;
    if (input.flags & 8u) != 0u {
        // Fragment position is the exact device-pixel center. Vertex-interpolated
        // UVs can round differently after translating or clipping the same image,
        // producing periodic one-byte lines between otherwise identical tiles.
        let local = (input.clip.xy - input.rect.xy) / input.rect.zw;
        uv = input.crop.xy + local * (input.crop.zw - input.crop.xy);
    }
    let at = uv * vec2<f32>(extent) - vec2<f32>(0.5);
    let low = vec2<i32>(floor(at));
    let weight = fract(at);
    // Each tap is already premultiplied, including color pixels bordering holes.
    let a = sample_texel(low, input.flags);
    let b = sample_texel(low + vec2<i32>(1, 0), input.flags);
    let c = sample_texel(low + vec2<i32>(0, 1), input.flags);
    let d = sample_texel(low + vec2<i32>(1, 1), input.flags);
    return mix(mix(a, b, weight.x), mix(c, d, weight.x), weight.y);
}
// Returns premultiplied color to the common One/OneMinusSrcAlpha blend stage.
@fragment fn raster_fragment(input: RasterOutput) -> @location(0) vec4<f32> {
    if (input.flags & 10u) != 0u {
        // Images request filtering without selecting glyph effect-table transforms.
        let ink = filtered(input);
        if (input.flags & 1u) == 0u || (input.flags & 4u) != 0u {
            // Masks and explicit replacement colors use filtered alpha with the tint.
            return input.tint * ink.a;
        }
        // Authored color survives an opacity-only effect without RGB tinting.
        return ink * input.tint.a;
    }
    if (input.flags & 1u) != 0u {
        let color = textureSampleLevel(color_atlas, atlas_sampler, input.uv, i32((input.flags >> 8u) & 255u), 0.0);
        // The ordinary nearest branch preserves existing emoji pixels exactly.
        return vec4<f32>(color.rgb * color.a, color.a);
    }
    // Explicit mip zero avoids derivatives in the per-instance sampling branch.
    let coverage = textureSampleLevel(mask_atlas, atlas_sampler, input.uv, i32((input.flags >> 8u) & 255u), 0.0).r;
    return input.tint * coverage;
}
