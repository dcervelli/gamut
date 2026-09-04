// Builds one level of the coarse chain (see render::reduce).
//
// Each destination texel is the exact area average of the STEP x STEP source
// texels under it, clipped to the source's extent so that an image whose size
// is not a multiple of STEP averages the edge over what is actually there
// rather than over padding.

struct Params {
    // How much of the source texture the image occupies, in source texels.
    // Fractional from the second level on, since each level rounds its size up.
    extent: vec2<f32>,
    step: f32,
    swizzle: u32,     // 0 gray, 1 gray+alpha, 2 rgb, 3 rgba
    alpha_mode: u32,  // 0 opaque, 1 straight, 2 premultiplied
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(1) @binding(0) var source: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

// Color premultiplied by alpha, in the component layout the texture stores.
// Averaging straight alpha would drag the color of fully transparent texels
// into its neighbours, which is what shows as haloing along a hard edge.
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

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let low = floor(position.xy) * params.step;
    let high = min(low + vec2<f32>(params.step), params.extent);

    let first = vec2<i32>(low);
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
