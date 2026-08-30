// Combines the linear image target and the sRGB UI target onto the surface,
// applying tone mapping and whatever encoding the output needs.
//
// This is the only stage that knows what the display can accept, which is why
// neither the image layer nor the UI layer has to.

struct Params {
    tone_map: u32,     // 0 clip, 1 reinhard, 2 neutral
    encoding: u32,     // 0 sRGB surface (hardware encodes), 1 scRGB linear, 2 PQ
    // Global output gain, applied after compositing. 1.0 means "1.0 is SDR
    // reference white", which is what both sRGB and scRGB want.
    white_scale: f32,
    // Side of one checkerboard square, in surface pixels.
    checker: f32,
    // The backdrop, in the same linear units the UI target holds: `base`
    // everywhere, with `alternate` taking every other square inside a region.
    base: vec4<f32>,
    alternate: vec4<f32>,
    // Where the checkerboard shows through: the image, and the minimap's
    // thumbnail when it is on screen. (left, top, right, bottom) in surface
    // pixels; an empty rectangle is one that is not being drawn this frame,
    // which the half-open test below rejects without needing a count.
    regions: array<vec4<f32>, 2>,
};

@group(0) @binding(0) var<uniform> params: Params;
// Read by texel index rather than sampled: both targets are exactly the size
// of the surface, so there is nothing to filter.
@group(1) @binding(0) var image_target: texture_2d<f32>;
@group(1) @binding(1) var ui_target: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // One oversized triangle covering the viewport.
    let x = f32(i32(index) / 2) * 4.0 - 1.0;
    let y = f32(i32(index) & 1) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn reinhard(color: vec3<f32>) -> vec3<f32> {
    return color / (color + vec3<f32>(1.0));
}

// Khronos PBR Neutral. Holds hue and saturation far better than a Reinhard
// curve and avoids the colour cast of the ACES approximations.
fn neutral(color_in: vec3<f32>) -> vec3<f32> {
    let start_compression = 0.8 - 0.04;
    let desaturation = 0.15;

    var color = color_in;
    let darkest = min(color.r, min(color.g, color.b));
    var offset = 0.04;
    if darkest < 0.08 {
        offset = darkest - 6.25 * darkest * darkest;
    }
    color = color - vec3<f32>(offset);

    let peak = max(color.r, max(color.g, color.b));
    if peak < start_compression {
        return color;
    }

    let d = 1.0 - start_compression;
    let new_peak = 1.0 - d * d / (peak + d - start_compression);
    color = color * (new_peak / peak);

    let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    return mix(color, vec3<f32>(new_peak), g);
}

// Mirrored on the CPU by `ToneMap::apply` in image/display.rs, for the one
// pixel the readout in the bottom bar has to describe.
fn tone_map(color: vec3<f32>) -> vec3<f32> {
    switch params.tone_map {
        case 1u: { return reinhard(max(color, vec3<f32>(0.0))); }
        case 2u: { return neutral(max(color, vec3<f32>(0.0))); }
        default: { return clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)); }
    }
}

// SMPTE ST 2084, taking a value relative to 203-nit reference white.
fn pq_encode(relative: vec3<f32>) -> vec3<f32> {
    let m1 = 2610.0 / 16384.0;
    let m2 = 128.0 * 2523.0 / 4096.0;
    let c1 = 3424.0 / 4096.0;
    let c2 = 32.0 * 2413.0 / 4096.0;
    let c3 = 32.0 * 2392.0 / 4096.0;
    let y = clamp(relative * 203.0 / 10000.0, vec3<f32>(0.0), vec3<f32>(1.0));
    let ym1 = pow(y, vec3<f32>(m1));
    return pow((vec3<f32>(c1) + c2 * ym1) / (vec3<f32>(1.0) + c3 * ym1), vec3<f32>(m2));
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let coord = vec2<i32>(position.xy);
    let image = textureLoad(image_target, coord, 0);
    let ui = textureLoad(ui_target, coord, 0);

    // The backdrop is authored at display brightness, like the interface, so
    // it goes under the image rather than through the tone curve with it.
    var color = backdrop(position.xy);

    // Both layers arrive premultiplied, so each is a plain source-over in
    // linear space. Tone mapping only ever applies to the image: the
    // interface must not be squashed along with it.
    color = color * (1.0 - image.a) + tone_map_premultiplied(image);
    color = color * (1.0 - ui.a) + ui.rgb;
    color = color * params.white_scale;

    switch params.encoding {
        // The surface format is *UnormSrgb; the hardware encodes on write.
        case 0u: { return vec4<f32>(color, 1.0); }
        // scRGB: linear values, 1.0 is SDR white, above that is brighter.
        case 1u: { return vec4<f32>(color, 1.0); }
        default: { return vec4<f32>(pq_encode(color), 1.0); }
    }
}

// What shows through wherever the image is transparent. The squares are laid
// out on the surface rather than on the image, so panning slides the image
// over a pattern that stays put instead of dragging it along.
fn backdrop(point: vec2<f32>) -> vec3<f32> {
    // A guard on the division below, not a case that arises: the caller sends
    // at least one pixel.
    if params.checker <= 0.0 {
        return params.base.rgb;
    }
    for (var index = 0u; index < 2u; index = index + 1u) {
        let region = params.regions[index];
        if all(point >= region.xy) && all(point < region.zw) {
            let cell = vec2<i32>(floor(point / params.checker));
            if ((cell.x + cell.y) & 1) == 1 {
                return params.alternate.rgb;
            }
            return params.base.rgb;
        }
    }
    return params.base.rgb;
}

// The curve acts on the colour, not on the colour faded by its coverage, so a
// half-transparent highlight tone maps to the same shade as an opaque one.
fn tone_map_premultiplied(texel: vec4<f32>) -> vec3<f32> {
    if texel.a <= 0.0 {
        return vec3<f32>(0.0);
    }
    if texel.a >= 1.0 {
        return tone_map(texel.rgb);
    }
    return tone_map(texel.rgb / texel.a) * texel.a;
}
