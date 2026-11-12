// Pixel-aligned terminal rectangles and rounded strokes (design §10.3).
//
// The CPU supplies device-pixel coordinates; one viewport uniform converts
// them to clip space. A signed distance is used only for rounded edges or
// strokes, while sharp integer edges remain exactly pixel aligned.

// Physical target dimensions protect the vertex transform from CSS scaling.
struct Viewport { size: vec2<f32>, padding: vec2<f32> }
@group(0) @binding(0) var<uniform> viewport: Viewport;

// Inputs mirror the 32-byte puzzle_frame::RectInstance byte layout.
struct RectInput {
    // Left, top, width and height in device pixels.
    @location(0) rect: vec4<f32>,
    // Low-to-high bytes hold straight red, green, blue and alpha.
    @location(1) color: u32,
    // Outer corner radius in device pixels.
    @location(2) radius: f32,
    // Zero means fill; otherwise retain only an inward stroke.
    @location(3) stroke: f32,
    // Future rectangle options; currently reserved.
    @location(4) flags: u32,
}

// Interpolated local position evaluates the same SDF at each fragment.
struct RectOutput {
    // Homogeneous clip position for WebGPU rasterization.
    @builtin(position) clip: vec4<f32>,
    // Position relative to the rectangle's top-left device pixel.
    @location(0) local: vec2<f32>,
    // Rectangle extent in device pixels.
    @location(1) extent: vec2<f32>,
    // Rounded outer edge radius.
    @location(2) radius: f32,
    // Inward stroke thickness.
    @location(3) stroke: f32,
    // Color is constant across a rectangle's two triangles.
    @location(4) @interpolate(flat) color: u32,
}

// Six corners make two triangles without an index buffer or per-quad vertices.
@vertex fn rect_vertex(@builtin(vertex_index) vertex: u32, input: RectInput) -> RectOutput {
    let corners = array<vec2<f32>, 6>(vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0));
    let local = corners[vertex] * input.rect.zw;
    let device = input.rect.xy + local;
    var output: RectOutput;
    output.clip = vec4<f32>(device.x * 2.0 / viewport.size.x - 1.0,
        1.0 - device.y * 2.0 / viewport.size.y, 0.0, 1.0);
    output.local = local;
    output.extent = input.rect.zw;
    output.radius = input.radius;
    output.stroke = input.stroke;
    output.color = input.color;
    return output;
}

// Analytic rounded-box distance; negative values are inside the fill.
fn rounded_distance(local: vec2<f32>, extent: vec2<f32>, radius: f32) -> f32 {
    let half = extent * 0.5;
    let r = min(radius, min(half.x, half.y));
    let q = abs(local - half) - (half - vec2<f32>(r));
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// A one-pixel ramp gives fractional round corners without MSAA.
@fragment fn rect_fragment(input: RectOutput) -> @location(0) vec4<f32> {
    let distance = rounded_distance(input.local, input.extent, input.radius);
    let outer = clamp(0.5 - distance, 0.0, 1.0);
    var coverage = outer;
    if input.stroke > 0.0 {
        // Subtract the inset fill, leaving a stroke within the outer edge.
        coverage = outer - clamp(0.5 - (distance + input.stroke), 0.0, 1.0);
    }
    let packed = input.color;
    let rgba = vec4<f32>(f32(packed & 0xffu), f32((packed >> 8u) & 0xffu),
        f32((packed >> 16u) & 0xffu), f32(packed >> 24u)) / 255.0;
    let alpha = rgba.a * coverage;
    return vec4<f32>(rgba.rgb * alpha, alpha);
}
