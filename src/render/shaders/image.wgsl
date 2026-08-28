// Draws the image into the linear working-space target.
//
// The texture is guaranteed to hold linear values (see render::upload), so
// there is no transfer function here. What remains is: expand whatever
// component layout we uploaded to RGBA, get into the BT.709 working space,
// apply the display window, and optionally false-colour a single channel.

struct Params {
    offset: vec2<f32>,      // top-left of the quad, in clip space
    scale: vec2<f32>,       // its size, in clip space
    window: vec2<f32>,      // (low, gain): displayed = (value - low) * gain
    _pad: vec2<f32>,
    primaries: mat3x3<f32>, // source primaries -> BT.709
    swizzle: u32,           // 0 gray, 1 gray+alpha, 2 rgb, 3 rgba
    alpha_mode: u32,        // 0 opaque, 1 straight, 2 premultiplied
    colormap: u32,          // 0 none, 1 viridis, 2 magma, 3 turbo
    _pad2: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(1) @binding(0) var source: texture_2d<f32>;
@group(1) @binding(1) var source_sampler: sampler;

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

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let cutoff = step(c, vec3<f32>(0.04045));
    let low = c / 12.92;
    let high = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return mix(high, low, cutoff);
}

// Polynomial fits to the matplotlib colormaps. Approximations, but well
// within what the eye resolves in a false-colour display. They produce
// sRGB-encoded values, so the caller linearises.
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
    let texel = textureSample(source, source_sampler, in.uv);

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

    // Undo premultiplication before the colour maths, so windowing and the
    // primaries matrix act on the actual colour rather than a faded one.
    if params.alpha_mode == 2u && alpha > 0.0 {
        color = color / alpha;
    }
    if params.alpha_mode == 0u {
        alpha = 1.0;
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
