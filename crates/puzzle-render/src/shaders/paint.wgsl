// COLR paint-table evaluation after faithful Slug coverage (design §9.5).
// Stops interpolate premultiplied sRGB; fragment compositing receives straight
// RGB plus alpha, preserving the existing Slug output convention.
// Brush transforms and stop interpolation do not modify Slug curve coverage.
// Clip records reference the same retained CPU band and curve mirrors.

// Loads one texel from a 256-wide row without a filtering sampler.
fn paint_load(row: i32, x: i32) -> vec4<f32> {
    return textureLoad(paint_texture, vec2<i32>(x, row), 0);
}

// Solves the moving-circle envelope and rejects negative-radius roots.
// The second component is one for a valid root and zero for transparent ink.
fn paint_radial(p: vec2<f32>, circles: vec4<f32>, radii: vec2<f32>) -> vec2<f32> {
    // Root coordinates have already been mapped into the brush local space.
    // Circle centers and radii use the same em units as that sample.
    let q = p - circles.xy;
    let d = circles.zw - circles.xy;
    let dr = radii.y - radii.x;
    // Expanding |q - t*d|² = (r0 + t*dr)² yields this quadratic.
    // Its roots locate circles whose circumference passes through the pixel.
    let a = dot(d, d) - dr * dr;
    let b = -2.0 * (dot(q, d) + radii.x * dr);
    let c = dot(q, q) - radii.x * radii.x;
    // Equal motion/radius growth degenerates to a linear equation.
    // This threshold matches the independent CPU brush evaluator.
    if abs(a) < 1e-12 {
        if abs(b) < 1e-12 { return vec2<f32>(0.0); }
        let t = -c / b;
        return vec2<f32>(t, select(0.0, 1.0, radii.x + t * dr >= 0.0));
    }
    // A negative discriminant means no circle contains a boundary sample.
    // Such pixels are transparent rather than extending an undefined color.
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 { return vec2<f32>(0.0); }
    let root = sqrt(disc);
    let t0 = (-b - root) / (2.0 * a);
    let t1 = (-b + root) / (2.0 * a);
    // Negative-radius circles are outside the COLR radial domain.
    // If both roots are valid, the later circle defines the visible envelope.
    let valid0 = radii.x + t0 * dr >= 0.0;
    let valid1 = radii.x + t1 * dr >= 0.0;
    if valid0 && valid1 { return vec2<f32>(max(t0, t1), 1.0); }
    if valid0 { return vec2<f32>(t0, 1.0); }
    if valid1 { return vec2<f32>(t1, 1.0); }
    return vec2<f32>(0.0);
}

// Extends an offset relative to the actual first and last stop positions.
fn paint_extend(t: f32, first: f32, last: f32, mode: i32) -> f32 {
    // Stops need not cover [0,1]; extend repeats their actual interval.
    // Pad keeps the offset unchanged because the color-line search clamps it.
    let span = last - first;
    if span <= 0.0 || mode == 0 { return t; }
    // floor-based wrapping also handles negative offsets consistently.
    let u = (t - first) / span;
    if mode == 1 { return first + span * (u - floor(u)); }
    // Euclidean modulo is needed for reflected intervals before offset zero.
    let cycle = u - 2.0 * floor(u / 2.0);
    return first + span * (1.0 - abs(cycle - 1.0));
}

// Searches at most sixteen stops, keeping exact duplicate offsets as jumps.
fn paint_line(row: i32, offset: f32, header: vec4<f32>, foreground: vec4<f32>) -> vec4<f32> {
    // Encoded rows cap the stop count at sixteen before GPU upload.
    let count = i32(header.z);
    if count == 0 { return vec4<f32>(0.0); }
    let first = paint_load(row, 8).x;
    let last = paint_load(row, 8 + (count - 1) * 2).x;
    // One stop is constant regardless of geometry or extension mode.
    if count == 1 { return paint_stop(row, 0, foreground); }
    // Repeating a zero-width interval has no drawable domain.
    if last <= first && header.y != 0.0 { return vec4<f32>(0.0); }
    let t = paint_extend(offset, first, last, i32(header.y));
    var before = first;
    var color = paint_stop(row, 0, foreground);
    if t < first { return color; }
    // Strict inequality chooses the last stop at an exact duplicated offset.
    // Premultiplied interpolation prevents transparent colors from leaking RGB.
    for (var i = 1; i < count; i += 1) {
        let after = paint_load(row, 8 + i * 2).x;
        let next = paint_stop(row, i, foreground);
        if t < after { return color + ((t - before) / (after - before)) * (next - color); }
        before = after;
        color = next;
    }
    return color;
}

// Samples the geometry in brush-local em coordinates.
fn paint_brush(row: i32, root: vec2<f32>, header: vec4<f32>, foreground: vec4<f32>) -> vec4<f32> {
    // Negative kind tags distinguish COLR records from the older RGBA probe.
    if header.x == -1.0 {
        let color = paint_load(row, 5);
        if paint_load(row, 6).x == 1.0 { return vec4<f32>(foreground.rgb * foreground.a, foreground.a) * color.a; }
        return color;
    }
    // Inverse affine coefficients map root-em coordinates into brush-em units.
    // Transforming samples instead of geometry also covers shear and rotation.
    let m = paint_load(row, 1);
    let p = vec2<f32>(m.x * root.x + m.z * root.y, m.y * root.x + m.w * root.y) + paint_load(row, 2).xy;
    let geometry = paint_load(row, 3);
    let params = paint_load(row, 4);
    var offset = 0.0;
    if header.x == -2.0 {
        let d = geometry.zw - geometry.xy;
        // Projection along the color-line axis ignores perpendicular distance.
        let norm = dot(d, d);
        if norm == 0.0 { return vec4<f32>(0.0); }
        offset = dot(p - geometry.xy, d) / norm;
    } else if header.x == -3.0 {
        let radial = paint_radial(p, geometry, params.xy);
        if radial.y == 0.0 { return vec4<f32>(0.0); }
        offset = radial.x;
    } else {
        let q = p - geometry.xy;
        // COLR sweep angles advance clockwise; font coordinates point upward.
        // Map the angle to one turn before applying the authored start/end range.
        let degrees = atan2(-q.y, q.x) * (180.0 / 3.141592653589793);
        let angle = degrees - 360.0 * floor(degrees / 360.0);
        let span = params.y - params.x;
        if span == 0.0 { return vec4<f32>(0.0); }
        offset = (angle - params.x) / span;
    }
    return paint_line(row, offset, header, foreground);
}

// Nested clips reuse the unchanged reference ray and coverage functions.
// Derivatives are supplied from uniform fragment flow before texture branches.
fn paint_clip(row: i32, at: vec2<f32>, width: vec2<f32>, count: i32) -> f32 {
    var coverage = 1.0;
    for (var i = 0; i < count; i += 1) {
        let bbox = paint_load(row, 40 + i * 2);
        let data = paint_load(row, 41 + i * 2);
        // The float texel stores band flags as bits, never as a numeric float.
        // Retaining this word preserves the exact reference band maxima.
        let flags = bitcast<u32>(data.z);
        let max_band = vec2<i32>(i32(flags & 255u), i32((flags >> 16u) & 255u));
        // Each enclosing clip has its own bounding-box band partition.
        let scale = vec2<f32>(max_band + vec2<i32>(1)) / (bbox.zw - bbox.xy);
        let band = clamp(vec2<i32>(at * scale - bbox.xy * scale), vec2<i32>(0), max_band);
        let horizontal = slug_ray_horizontal(at, 1.0 / width, vec2<i32>(data.xy), band.y);
        let vertical = slug_ray_vertical(at, 1.0 / width, vec2<i32>(data.xy), max_band.y, band.x);
        // Multiply independent clip coverage after the leaf coverage stage.
        // Ray math and winding flags remain the faithful Slug implementation.
        coverage *= CalcCoverage(horizontal.coverage, vertical.coverage, horizontal.weight, vertical.weight, i32(flags >> 16u));
    }
    return coverage;
}

// Zero keeps the instance color; legacy positive headers retain the M3 probe.
fn paint(input: VertexOutput, width: vec2<f32>) -> vec4<f32> {
    if input.paint == 0u { return input.color; }
    // Nonzero indices use the existing plus-one instance convention.
    let index = input.paint - 1u;
    let row = i32(index / 256u);
    let header = paint_load(row, i32(index % 256u));
    if header.x >= 0.0 { return header; }
    let color = paint_brush(row, input.texcoord, header, input.color);
    let clip = paint_clip(row, input.texcoord, width, i32(header.w));
    if color.a == 0.0 { return vec4<f32>(0.0); }
    // The caller premultiplies after Slug coverage, so return straight RGB.
    return vec4<f32>(color.rgb / color.a, color.a * clip);
}

// A palette stop ignores tint; the foreground sentinel inherits text RGB.
fn paint_stop(row: i32, i: i32, foreground: vec4<f32>) -> vec4<f32> {
    let color = paint_load(row, 9 + i * 2);
    // Only the foreground sentinel participates in selection or cursor tint.
    if paint_load(row, 8 + i * 2).y == 1.0 {
        return vec4<f32>(foreground.rgb * foreground.a, foreground.a) * color.a;
    }
    return color;
}
