// ===================================================
// WGSL port of the reference vertex and pixel shaders for the Slug algorithm.
// Reference vertex shader for the Slug algorithm.
// Reference pixel shader for the Slug algorithm.
// This code is made available under the MIT License.
// COLR gradient colors and enclosing clips are evaluated by paint.wgsl
// after coverage; root solving, band walks and coverage formulas are unchanged.
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright 2017, by Eric Lengyel.
// Reference commit be3c13eb7d63f9e8aa5c583e42d92c374cb91d98.
// ===================================================
// Deviations allowed by design §9.4: Rgba32Float curve data (D-8), runtime
// E/W flags, instanced vertex expansion, and post-coverage paint. The math
// and constants in SlugDilate and the pixel functions retain reference order.

// The curve and band textures use a fixed width of 4096 texels.
const kLogBandTextureWidth: u32 = 12u;
// It's convenient to have a texel load function to aid in translation to
// other shader languages. WGSL's textureLoad supplies that operation here.

// The four rows of the MVP matrix and the viewport dimensions, in pixels.
// Object space is logical pixels; the matrix includes DPR and view transforms.
struct SlugParams {
    matrix: array<vec4<f32>, 4>,
    viewport: vec4<f32>,
}
@group(0) @binding(0) var<uniform> slug: SlugParams;
// The reference's float and integer texel loads have no sampler or filtering.
@group(0) @binding(1) var curve_texture: texture_2d<f32>;
@group(0) @binding(2) var band_texture: texture_2d<u32>;
// M11 stores 256-wide COLR gradient/clip rows; zero bypasses this binding.
@group(0) @binding(3) var paint_texture: texture_2d<f32>;

// Five reference float4 attributes are expanded from one 56-byte instance.
// Packed integers stay integers here; WGSL does not need HLSL's asuint cast.
// The per-vertex input data consists of 5 attributes all having 4
// floating-point components in the reference: pos, tex, jac, bnd and col.
// pos.xy = object-space vertex coordinates.
// pos.zw = object-space normal vector.
// tex.xy = em-space sample coordinates.
// tex.z = location of glyph data in band texture (interpreted as integer).
// tex.w = max band indexes and flags (interpreted as integer).
// The reference's tex.z word gives the x coordinate in bits 0–15 and the
// y coordinate in bits 16–31. tex.w puts band max x in bits 0–7, band max y
// in bits 16–23 and even-odd flag E in bit 28; this port adds W at bit 29.
// The reference's five float4 inputs are numbered 0=pos, 1=tex, 2=jac,
// 3=bnd and 4=col. Instancing derives those same values per vertex here.
// jac = inverse Jacobian matrix entries (00, 01, 10, 11).
// bnd = (band scale x, band scale y, band offset x, band offset y).
// col = vertex color (red, green, blue, alpha).
struct VertexInput {
    @location(0) m: vec4<f32>,
    @location(1) t: vec2<f32>,
    @location(2) bbox: vec4<f32>,
    @location(3) glyph_loc: u32,
    @location(4) band_info: u32,
    @location(5) color: u32,
    @location(6) paint: u32,
};
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) texcoord: vec2<f32>,
    // Band transforms and packed addresses are constant across one quad.
    @location(2) @interpolate(flat) banding: vec4<f32>,
    @location(3) @interpolate(flat) glyph: vec4<i32>,
    @location(4) @interpolate(flat) paint: u32,
};
struct Dilated {
    // Em-space sample coordinate after pixel-sized outward dilation.
    texcoord: vec2<f32>,
    // Object-space position passed through the MVP rows.
    position: vec2<f32>,
};

// tex.z is glyph data location: low 16 bits x, high 16 bits y.
// tex.w is band maxima and flags: E in bit 28 and W in bit 29.
// The reference packs band x in bits 0–7, band y in bits 16–23 and E in
// bit 28. W occupies the following bit in this runtime-flag extension.
fn SlugUnpack(glyph_loc: u32, band_info: u32) -> vec4<i32> {
    return vec4<i32>(i32(glyph_loc & 0xffffu), i32(glyph_loc >> 16u),
                     i32(band_info & 0xffffu), i32(band_info >> 16u));
}

// Apply dynamic dilation to vertex position. Returns new em-space sample position.
// pos.xy is the object-space corner and pos.zw its outward direction.
// tex.xy is the em-space corner; jac is the inverse object transform.
// The dilation is computed before the MVP projection. This lets rotated or
// sheared quads expand in object space while preserving em-space sampling.
fn SlugDilate(pos: vec4<f32>, tex: vec2<f32>, jac: vec4<f32>,
              m0: vec4<f32>, m1: vec4<f32>, m3: vec4<f32>,
              dim: vec2<f32>) -> Dilated {
    let n = normalize(pos.zw);
    // s and t describe the homogeneous projection at the corner and in
    // the outward direction. In an orthographic view, s=1 and t=0.
    let s = dot(m3.xy, pos.xy) + m3.w;
    let t = dot(m3.xy, n);
    let u = (s * dot(m0.xy, n) - t * (dot(m0.xy, pos.xy) + m0.w)) * dim.x;
    let v = (s * dot(m1.xy, n) - t * (dot(m1.xy, pos.xy) + m1.w)) * dim.y;
    let s2 = s * s;
    let st = s * t;
    let uv = u * u + v * v;
    // Keep the reference rational expression; its denominator accounts
    // for projected dilation under a perspective MVP as well.
    let d = pos.zw * (s2 * (st + sqrt(uv)) / (uv - st * st));
    let vpos = pos.xy + d;
    return Dilated(vec2<f32>(tex.x + dot(d, jac.xy), tex.y + dot(d, jac.zw)), vpos);
}

// Expand static quad vertex 0–3 to the reference's pos, tex, jac and bnd.
@vertex
fn slug_vertex(input: VertexInput, @builtin(vertex_index) vid: u32) -> VertexOutput {
    // Bits of the static vertex index select the em-space bbox corner.
    let high_x = (vid & 1u) != 0u;
    let high_y = (vid & 2u) != 0u;
    let corner = vec2<f32>(select(input.bbox.x, input.bbox.z, high_x),
                           select(input.bbox.y, input.bbox.w, high_y));
    let sign = vec2<f32>(select(-1.0, 1.0, high_x), select(-1.0, 1.0, high_y));
    // The sign pair points outside the outline box at each corner. The
    // instance transform supplies the direction in object-space pixels.
    // Column-major m maps em coordinates into logical-pixel object space.
    let pos_xy = vec2<f32>(input.m.x * corner.x + input.m.z * corner.y + input.t.x,
                            input.m.y * corner.x + input.m.w * corner.y + input.t.y);
    let normal = vec2<f32>(input.m.x * sign.x + input.m.z * sign.y,
                            input.m.y * sign.x + input.m.w * sign.y);
    let determinant = input.m.x * input.m.w - input.m.z * input.m.y;
    // The inverse Jacobian converts the dilation vector back into ems,
    // so the new vertex still samples the same quadratic outline.
    let jac = vec4<f32>(input.m.w, -input.m.z, -input.m.y, input.m.x) / determinant;
    let dilated = SlugDilate(vec4<f32>(pos_xy, normal), corner, jac,
                             slug.matrix[0], slug.matrix[1], slug.matrix[3], slug.viewport.xy);
    var result: VertexOutput;
    // Apply MVP matrix to dilated vertex position.
    // The same four rows are evaluated after dilation as in the reference.
    result.position.x = dilated.position.x * slug.matrix[0].x + dilated.position.y * slug.matrix[0].y + slug.matrix[0].w;
    result.position.y = dilated.position.x * slug.matrix[1].x + dilated.position.y * slug.matrix[1].y + slug.matrix[1].w;
    result.position.z = dilated.position.x * slug.matrix[2].x + dilated.position.y * slug.matrix[2].y + slug.matrix[2].w;
    result.position.w = dilated.position.x * slug.matrix[3].x + dilated.position.y * slug.matrix[3].y + slug.matrix[3].w;
    result.texcoord = dilated.texcoord;
    // Bands span the tight em bbox; scale and offset match the CPU oracle's
    // at*scale+offset operation order at floating-point band boundaries.
    let maximum = vec2<f32>(f32(input.band_info & 0xffu),
                             f32((input.band_info >> 16u) & 0xffu));
    let scale = (maximum + vec2<f32>(1.0)) / (input.bbox.zw - input.bbox.xy);
    let offset = -input.bbox.xy * scale;
    // Unpack or pass through remaining vertex data.
    result.banding = vec4<f32>(scale, offset);
    result.glyph = SlugUnpack(input.glyph_loc, input.band_info);
    // Packed RGBA bytes are straight sRGB colors; the fragment output is
    // premultiplied only after analytic coverage and alpha are known.
    result.color = vec4<f32>(f32(input.color & 0xffu), f32((input.color >> 8u) & 0xffu),
                              f32((input.color >> 16u) & 0xffu), f32(input.color >> 24u)) / 255.0;
    result.paint = input.paint;
    return result;
}

// Calculate the root eligibility code for a sample-relative quadratic Bézier curve.
// Extract the signs of the y coordinates of the three control points.
fn CalcRootCode(y1: f32, y2: f32, y3: f32) -> u32 {
    let i1 = bitcast<u32>(y1) >> 31u;
    let i2 = bitcast<u32>(y2) >> 30u;
    let i3 = bitcast<u32>(y3) >> 29u;
    // Sign bits form a compact lookup index; the fixed 0x2E74 code returns
    // which directed root crossings can contribute to winding.
    var shift = (i2 & 2u) | (i1 & ~2u);
    shift = (i3 & 4u) | (shift & ~4u);
    // Eligibility is returned in bits 0 and 8.
    return (0x2e74u >> shift) & 0x0101u;
}

// Solve for the values of t where the curve crosses y = 0.
// Solve for t where the curve crosses y = 0. The polynomial is a*t²−2b*t+c,
// with a=p1−2p2+p3, b=p1−p2 and c=p1. Imaginary roots become a double root
// at the global minimum t=b/a, matching the reference's clamped discriminant.
// The quadratic polynomial in t is given by a t^2 - 2b t + c. The
// discriminant b^2 - ac is clamped to zero; imaginary roots are treated
// as a double root at the global minimum, where t = b / a.
// Here a = p1.y - 2 p2.y + p3.y, b = p1.y - p2.y and c = p1.y.
fn SolveHorizPoly(p12: vec4<f32>, p3: vec2<f32>) -> vec2<f32> {
    let a = p12.xy - p12.zw * 2.0 + p3;
    let b = p12.xy - p12.zw;
    let ra = 1.0 / a.y;
    let rb = 0.5 / b.y;
    let d = sqrt(max(b.y * b.y - a.y * p12.y, 0.0));
    var t1 = (b.y - d) * ra;
    var t2 = (b.y + d) * ra;
    // If the polynomial is nearly linear, solve −2b*t+c=0. The unused
    // division by zero above stays in reference order, per design §9.4.
    if abs(a.y) < 1.0 / 65536.0 { t1 = p12.y * rb; t2 = t1; }
    // Return the x coordinates where C(t)=0.
    return vec2<f32>((a.x * t1 - b.x * 2.0) * t1 + p12.x,
                     (a.x * t2 - b.x * 2.0) * t2 + p12.x);
}

// Solve for the values of t where the curve crosses x = 0.
fn SolveVertPoly(p12: vec4<f32>, p3: vec2<f32>) -> vec2<f32> {
    let a = p12.xy - p12.zw * 2.0 + p3;
    let b = p12.xy - p12.zw;
    let ra = 1.0 / a.x;
    let rb = 0.5 / b.x;
    let d = sqrt(max(b.x * b.x - a.x * p12.x, 0.0));
    // This exchanges axes only; the reference uses the same threshold and
    // deliberately computes reciprocal a before the near-linear branch.
    var t1 = (b.x - d) * ra;
    var t2 = (b.x + d) * ra;
    // If the polynomial is nearly linear, solve −2b*t+c=0.
    if abs(a.x) < 1.0 / 65536.0 { t1 = p12.x * rb; t2 = t1; }
    // Return the y coordinates where C(t)=0.
    return vec2<f32>((a.y * t1 - b.y * 2.0) * t1 + p12.y,
                     (a.y * t2 - b.y * 2.0) * t2 + p12.y);
}

// If the offset causes the x coordinate to exceed the texture width, then wrap to the next line.
fn CalcBandLoc(glyph_loc: vec2<i32>, offset: u32) -> vec2<i32> {
    var loc = vec2<i32>(glyph_loc.x + i32(offset), glyph_loc.y);
    loc.y += loc.x >> kLogBandTextureWidth;
    // Curve lists use this wrapping base, unlike individual curve p3
    // reads and band headers, which the CPU packer keeps inside one row.
    loc.x &= (1 << kLogBandTextureWidth) - 1;
    return loc;
}

// Combine coverages from the horizontal and vertical rays using their weights.
// Absolute values ensure that either winding direction convention works.
// The reference also retains the smaller absolute ray estimate as a
// floor so an unweighted edge does not disappear near a grazing crossing.
fn CalcCoverage(xcov: f32, ycov: f32, xwgt: f32, ywgt: f32, flags: i32) -> f32 {
    var coverage = max(abs(xcov * xwgt + ycov * ywgt) / max(xwgt + ywgt, 1.0 / 65536.0),
                       min(abs(xcov), abs(ycov)));
    // If SLUG_EVENODD is defined during compilation, then check E flag in tex.w.
    // Runtime E flag replaces the reference's compile-time SLUG_EVENODD.
    if (flags & 0x1000) == 0 {
        // Using nonzero fill rule here.
        coverage = clamp(coverage, 0.0, 1.0);
    } else {
        // Using even-odd fill rule here.
        coverage = 1.0 - abs(1.0 - fract(coverage * 0.5) * 2.0);
    }
    // If SLUG_WEIGHT is defined during compilation, then take a square root to boost optical weight.
    // Runtime W flag replaces SLUG_WEIGHT's compile-time square root.
    if (flags & 0x2000) != 0 { coverage = sqrt(coverage); }
    return coverage;
}

// One reference ray's signed winding coverage and edge-proximity weight.
struct RayResult { coverage: f32, weight: f32, };

// The reference's horizontal loop, split out only for R7's function limit.
fn slug_ray_horizontal(render_coord: vec2<f32>, pixels_per_em: vec2<f32>,
                       glyph_loc: vec2<i32>, band_y: i32) -> RayResult {
    var xcov = 0.0;
    var xwgt = 0.0;
    // Fetch data for the horizontal band from the index texture. The number
    // of curves intersecting the band is in x; the offset to their list
    // of curve locations is in y. This is the reference header layout.
    let hband = textureLoad(band_texture, glyph_loc + vec2<i32>(band_y, 0), 0).xy;
    let hloc = CalcBandLoc(glyph_loc, hband.y);
    // Loop over all curves in the horizontal band. The first texel holds
    // p1 and p2, and its next texel holds p3, as in the HLSL reference.
    for (var curve_index = 0; curve_index < i32(hband.x); curve_index++) {
        // Fetch the location of the current curve from the index texture.
        // The list gives curve-texture coordinates. C1 keeps p3 at x+1.
        let curve_loc = vec2<i32>(textureLoad(band_texture,
                                  hloc + vec2<i32>(curve_index, 0), 0).xy);
        let p12 = textureLoad(curve_texture, curve_loc, 0) - vec4<f32>(render_coord, render_coord);
        let p3 = textureLoad(curve_texture, curve_loc + vec2<i32>(1, 0), 0).xy - render_coord;
        // The first texel contains both p1 and p2 in its (x,y) and (z,w)
        // components, respectively, and the second contains p3 in (x,y).
        // Subtracting render coordinates makes the curve sample-relative.
        // The quadratic Bézier is C(t)=(1−t)^2 p1 + 2t(1−t)p2 + t^2 p3.
        // If the largest x among all three control points lies left of the
        // pixel, no more curves in the horizontal band can influence it.
        // Descending maximum x makes that early exit correct; the reference
        // uses the negative half-pixel window before breaking the loop.
        if max(max(p12.x, p12.z), p3.x) * pixels_per_em.x < -0.5 { break; }
        let code = CalcRootCode(p12.y, p12.w, p3.y);
        if code != 0u {
            // At least one root contributes. Convert its crossing position
            // so this pixel corresponds to the range [0,1]. The code bits
            // tell which of the two roots contributes to signed coverage.
            let r = SolveHorizPoly(p12, p3) * pixels_per_em.x;
            if (code & 1u) != 0u {
                xcov += clamp(r.x + 0.5, 0.0, 1.0);
                xwgt = max(xwgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0));
            }
            if code > 1u {
                xcov -= clamp(r.y + 0.5, 0.0, 1.0);
                xwgt = max(xwgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0));
            }
        }
    }
    return RayResult(xcov, xwgt);
}

// The reference's vertical loop, split out only for R7's function limit.
fn slug_ray_vertical(render_coord: vec2<f32>, pixels_per_em: vec2<f32>,
                     glyph_loc: vec2<i32>, band_max_y: i32, band_x: i32) -> RayResult {
    var ycov = 0.0;
    var ywgt = 0.0;
    // Fetch vertical-band data after all horizontal headers, hence the
    // addition of bandMax.y + 1 to the packed header location.
    let vband = textureLoad(band_texture,
                 glyph_loc + vec2<i32>(band_max_y + 1 + band_x, 0), 0).xy;
    let vloc = CalcBandLoc(glyph_loc, vband.y);
    // Loop over all curves in the vertical band. Its signed winding
    // contributions reverse the horizontal convention after axis exchange.
    for (var curve_index = 0; curve_index < i32(vband.x); curve_index++) {
        let curve_loc = vec2<i32>(textureLoad(band_texture,
                                  vloc + vec2<i32>(curve_index, 0), 0).xy);
        let p12 = textureLoad(curve_texture, curve_loc, 0) - vec4<f32>(render_coord, render_coord);
        let p3 = textureLoad(curve_texture, curve_loc + vec2<i32>(1, 0), 0).xy - render_coord;
        // If the largest y among the control points lies below the pixel,
        // remaining curves cannot affect this vertical ray. The list is
        // sorted by descending maximum y, making this early exit valid.
        if max(max(p12.y, p12.w), p3.y) * pixels_per_em.y < -0.5 { break; }
        let code = CalcRootCode(p12.x, p12.z, p3.x);
        if code != 0u {
            let r = SolveVertPoly(p12, p3) * pixels_per_em.y;
            if (code & 1u) != 0u {
                ycov -= clamp(r.x + 0.5, 0.0, 1.0);
                ywgt = max(ywgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0));
            }
            if code > 1u {
                ycov += clamp(r.y + 0.5, 0.0, 1.0);
                ywgt = max(ywgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0));
            }
        }
    }
    return RayResult(ycov, ywgt);
}

// The reference's two-ray pixel coverage over packed 4096-wide texels.
fn SlugRender(render_coord: vec2<f32>, band_transform: vec4<f32>, glyph_data: vec4<i32>) -> f32 {
    // The derivative stays first, before either divergent helper loop.
    let ems_per_pixel = fwidth(render_coord);
    let pixels_per_em = 1.0 / ems_per_pixel;
    // The effective pixel dimensions of the em square are computed
    // independently for x and y using the texcoord derivatives.
    var band_max = glyph_data.zw;
    band_max.y &= 0x00ff;
    // Determine the pixel's horizontal and vertical bands by applying the
    // reference's band scale and offset to render coordinates. Clamp their
    // indexes to [0, bandMax.xy] after the float-to-int truncation.
    let band_index = clamp(vec2<i32>(render_coord * band_transform.xy + band_transform.zw),
                           vec2<i32>(0), band_max);
    let glyph_loc = glyph_data.xy;
    // These helpers contain the unchanged reference loops and no derivative.
    let horizontal = slug_ray_horizontal(render_coord, pixels_per_em, glyph_loc, band_index.y);
    let vertical = slug_ray_vertical(render_coord, pixels_per_em, glyph_loc,
                                      band_max.y, band_index.x);
    return CalcCoverage(horizontal.coverage, vertical.coverage,
                        horizontal.weight, vertical.weight, glyph_data.w);
}

@fragment
fn slug_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    // Uniform derivatives also serve any enclosing COLR clip outlines.
    let width = fwidth(input.texcoord);
    let coverage = SlugRender(input.texcoord, input.banding, input.glyph);
    let color = paint(input, width);
    let alpha = color.a * coverage;
    return vec4<f32>(color.rgb * alpha, alpha);
}
