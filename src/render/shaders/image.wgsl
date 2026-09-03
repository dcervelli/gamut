// Draws the image into the linear working-space target.
//
// The texture is guaranteed to hold linear values (see render::upload), so
// there is no transfer function here. What remains is: resample to the size
// the view asks for, expand whatever component layout we uploaded to RGBA, get
// into the BT.709 working space, apply the display window, and optionally
// false-colour a single channel.
//
// Resampling is done here with explicit texel loads rather than by a sampler.
// A sampler offers one bilinear tap, which neither shows the pixel grid a
// measurement image is read on nor covers more than four texels when the view
// is minifying; the three filters below do. `source` is level 0 of the image
// when magnifying and a coarse level (see render::reduce) when the view is
// zoomed far enough out to need one, so `extent` rather than the texture's own
// size says how much of it the image occupies.

struct Params {
    offset: vec2<f32>,           // top-left of the quad, in clip space
    scale: vec2<f32>,            // its size, in clip space
    window: vec2<f32>,           // (low, gain): displayed = (value - low) * gain
    texels_per_pixel: vec2<f32>, // source texels covered by one output pixel
    extent: vec2<f32>,           // image size, in the bound texture's texels
    _pad: vec2<f32>,
    primaries: mat3x3<f32>,      // source primaries -> BT.709
    swizzle: u32,                // 0 gray, 1 gray+alpha, 2 rgb, 3 rgba
    alpha_mode: u32,             // 0 opaque, 1 straight, 2 premultiplied
    colormap: u32,               // 0 none, 1 viridis, 2 magma, 3 turbo
    resampler: u32,                // 0 area, 1 antialiased nearest, 2 bicubic
};

@group(0) @binding(0) var<uniform> params: Params;
@group(1) @binding(0) var source: texture_2d<f32>;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    let uv = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    var out: VertexOut;
    out.uv = uv;
    out.position = vec4<f32>(
        params.offset.x + uv.x * params.scale.x,
        params.offset.y - uv.y * params.scale.y,
        0.0,
        1.0,
    );
    return out;
}

// Colour premultiplied by alpha, in the component layout the texture stores.
// Every filter below is a weighted sum of texels, and weighting straight alpha
// would drag the colour of fully transparent texels into their neighbours,
// which is what shows as haloing along a hard edge. Coarse levels are already
// premultiplied, so this is a no-op for them.
fn premultiplied(texel: vec4<f32>) -> vec4<f32> {
    if params.alpha_mode != 1u {
        return texel;
    }
    switch params.swizzle {
        case 1u: { return vec4<f32>(texel.r * texel.g, texel.g, texel.ba); }
        case 3u: { return vec4<f32>(texel.rgb * texel.a, texel.a); }
        default: { return texel; }
    }
}

fn load(coord: vec2<i32>) -> vec4<f32> {
    let limit = vec2<i32>(textureDimensions(source)) - vec2<i32>(1);
    return premultiplied(textureLoad(source, clamp(coord, vec2<i32>(0), limit), 0));
}

// Exact area average: every source texel under the output pixel, each weighted
// by how much of it the pixel actually covers. The count of taps is bounded by
// the coarse chain, which keeps `texels_per_pixel` at four or under however far
// the view zooms out; the clamp is a backstop, not a working limit.
fn area(uv: vec2<f32>) -> vec4<f32> {
    let half = min(params.texels_per_pixel, vec2<f32>(64.0)) * 0.5;
    let centre = uv * params.extent;
    let low = centre - half;
    let high = centre + half;

    let first = vec2<i32>(floor(low));
    let last = vec2<i32>(ceil(high)) - vec2<i32>(1);

    var sum = vec4<f32>(0.0);
    var total = 0.0;
    for (var y = first.y; y <= last.y; y = y + 1) {
        let wy = min(high.y, f32(y + 1)) - max(low.y, f32(y));
        if wy <= 0.0 {
            continue;
        }
        for (var x = first.x; x <= last.x; x = x + 1) {
            let wx = min(high.x, f32(x + 1)) - max(low.x, f32(x));
            if wx <= 0.0 {
                continue;
            }
            let weight = wx * wy;
            sum = sum + load(vec2<i32>(x, y)) * weight;
            total = total + weight;
        }
    }
    return sum / max(total, 1e-8);
}

// Nearest neighbour, except across the one output pixel that straddles a texel
// boundary, where it ramps instead of stepping. Keeps the pixel grid a
// measurement image is read on, without the uneven column doubling plain
// nearest gives at a zoom that is not a whole number.
fn antialiased_nearest(uv: vec2<f32>) -> vec4<f32> {
    let position = uv * params.extent - vec2<f32>(0.5);
    let base = floor(position);
    let offset = position - base;
    let width = max(params.texels_per_pixel, vec2<f32>(1e-6));
    let t = clamp((offset - vec2<f32>(0.5)) / width + vec2<f32>(0.5), vec2<f32>(0.0), vec2<f32>(1.0));

    let corner = vec2<i32>(base);
    let top = mix(load(corner), load(corner + vec2<i32>(1, 0)), t.x);
    let bottom = mix(load(corner + vec2<i32>(0, 1)), load(corner + vec2<i32>(1, 1)), t.x);
    return mix(top, bottom, t.y);
}

// Catmull-Rom, the B = 0, C = 1/2 member of the cubic family: interpolating,
// so texel centres come through untouched, and sharper than bilinear at the
// cost of a little ringing either side of a hard edge.
fn catmull_rom(offset: f32) -> array<f32, 4> {
    let f2 = offset * offset;
    let f3 = f2 * offset;
    return array<f32, 4>(
        (-f3 + 2.0 * f2 - offset) * 0.5,
        (3.0 * f3 - 5.0 * f2 + 2.0) * 0.5,
        (-3.0 * f3 + 4.0 * f2 + offset) * 0.5,
        (f3 - f2) * 0.5,
    );
}

fn bicubic(uv: vec2<f32>) -> vec4<f32> {
    let position = uv * params.extent - vec2<f32>(0.5);
    let base = floor(position);
    let offset = position - base;
    let wx = catmull_rom(offset.x);
    let wy = catmull_rom(offset.y);
    let corner = vec2<i32>(base) - vec2<i32>(1);

    var sum = vec4<f32>(0.0);
    for (var y = 0; y < 4; y = y + 1) {
        var row = vec4<f32>(0.0);
        for (var x = 0; x < 4; x = x + 1) {
            row = row + load(corner + vec2<i32>(x, y)) * wx[x];
        }
        sum = sum + row * wy[y];
    }
    return sum;
}

fn resample(uv: vec2<f32>) -> vec4<f32> {
    switch params.resampler {
        case 1u: { return antialiased_nearest(uv); }
        case 2u: { return bicubic(uv); }
        default: { return area(uv); }
    }
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let cutoff = step(c, vec3<f32>(0.04045));
    let low = c / 12.92;
    let high = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return mix(high, low, cutoff);
}

// Polynomial fits to the matplotlib colormaps. Approximations, but well
// within what the eye resolves in a false-colour display. They produce
// sRGB-encoded values, so the caller linearises.
//
// Viridis and magma are Matt Zucker's fits, from
// https://www.shadertoy.com/view/WlfXRN, under CC0; `REUSE.toml` records it.
//
// Mirrored on the CPU by `Colormap::color` in image/display.rs, which the
// pointer readout uses to say what colour a pixel came out. Change one, change
// the other: a swatch that disagrees with the screen is worse than no swatch.
fn viridis(t: f32) -> vec3<f32> {
    let c0 = vec3<f32>(0.2777273, 0.00540734, 0.33409980);
    let c1 = vec3<f32>(0.10509304, 1.40461353, 1.38459016);
    let c2 = vec3<f32>(-0.33086183, 0.21484756, 0.09509516);
    let c3 = vec3<f32>(-4.63423050, -5.79910097, -19.33244096);
    let c4 = vec3<f32>(6.22826994, 14.17993337, 56.69055260);
    let c5 = vec3<f32>(4.77638500, -13.74514538, -65.35303263);
    let c6 = vec3<f32>(-5.43545586, 4.64585261, 26.31241433);
    return c0 + t * (c1 + t * (c2 + t * (c3 + t * (c4 + t * (c5 + t * c6)))));
}

fn magma(t: f32) -> vec3<f32> {
    let c0 = vec3<f32>(-0.00213649, -0.00074966, -0.00538613);
    let c1 = vec3<f32>(0.25166054, 0.67752324, 2.49402660);
    let c2 = vec3<f32>(8.35371728, -3.57771951, 0.31446790);
    let c3 = vec3<f32>(-27.66873309, 14.26473078, -13.64921319);
    let c4 = vec3<f32>(52.17613981, -27.94360607, 12.94416944);
    let c5 = vec3<f32>(-50.76852536, 29.04658282, 4.23415299);
    let c6 = vec3<f32>(18.65570507, -11.48977352, -5.60196151);
    return c0 + t * (c1 + t * (c2 + t * (c3 + t * (c4 + t * (c5 + t * c6)))));
}

// Turbo's colormap is Anton Mikhailov's and this fit to it Ruofei Du's, both
// of Google, published under Apache-2.0; `REUSE.toml` records it.
// https://gist.github.com/mikhailov-work/0d177465a8151eb6ede1768d51d476c7
fn turbo(t: f32) -> vec3<f32> {
    let red4 = vec4<f32>(0.13572138, 4.61539260, -42.66032258, 132.13108234);
    let green4 = vec4<f32>(0.09140261, 2.19418839, 4.84296658, -14.18503333);
    let blue4 = vec4<f32>(0.10667330, 12.64194608, -60.58204836, 110.36276771);
    let red2 = vec2<f32>(-152.94239396, 59.28637943);
    let green2 = vec2<f32>(4.27729857, 2.82956604);
    let blue2 = vec2<f32>(-89.90310912, 27.34824973);

    let v4 = vec4<f32>(1.0, t, t * t, t * t * t);
    let v2 = v4.zw * v4.z;
    return vec3<f32>(
        dot(v4, red4) + dot(v2, red2),
        dot(v4, green4) + dot(v2, green2),
        dot(v4, blue4) + dot(v2, blue2),
    );
}

fn false_color(which: u32, t: f32) -> vec3<f32> {
    let clamped = clamp(t, 0.0, 1.0);
    var encoded: vec3<f32>;
    switch which {
        case 1u: { encoded = viridis(clamped); }
        case 2u: { encoded = magma(clamped); }
        case 3u: { encoded = turbo(clamped); }
        default: { encoded = vec3<f32>(clamped); }
    }
    return srgb_to_linear(clamp(encoded, vec3<f32>(0.0), vec3<f32>(1.0)));
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let texel = resample(in.uv);

    // Expand whatever we uploaded to RGBA. Grey replicates; alpha defaults
    // to opaque when the source had none.
    var color: vec3<f32>;
    var alpha: f32;
    switch params.swizzle {
        case 0u: { color = vec3<f32>(texel.r); alpha = 1.0; }
        case 1u: { color = vec3<f32>(texel.r); alpha = texel.g; }
        case 2u: { color = texel.rgb; alpha = 1.0; }
        default: { color = texel.rgb; alpha = texel.a; }
    }

    // Undo the premultiplication `resample` worked in, so that windowing and
    // the primaries matrix act on the actual colour rather than a faded one.
    // Bicubic's negative lobes can undershoot, so a texel that has resolved to
    // near-nothing is taken as nothing rather than divided into a wild colour.
    if params.alpha_mode == 0u {
        alpha = 1.0;
    } else {
        alpha = clamp(alpha, 0.0, 1.0);
        if alpha > 1e-4 {
            color = color / alpha;
        } else {
            color = vec3<f32>(0.0);
            alpha = 0.0;
        }
    }

    let is_gray = params.swizzle < 2u;
    if !is_gray {
        color = params.primaries * color;
    }

    // The display window. One control serving both HDR exposure and the
    // window/level a measurement image needs.
    let windowed = (color - vec3<f32>(params.window.x)) * params.window.y;

    var result: vec3<f32>;
    if is_gray && params.colormap != 0u {
        result = false_color(params.colormap, windowed.r);
    } else {
        result = windowed;
    }

    // The target blends with premultiplied alpha.
    return vec4<f32>(result * alpha, alpha);
}
