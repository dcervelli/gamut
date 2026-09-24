// Draws the image into the linear working-space target.
//
// The texture is guaranteed to hold linear values (see render::upload), so
// there is no transfer function here. What remains is: resample to the size
// the view asks for, expand whatever component layout we uploaded to RGBA, get
// into the BT.709 working space, apply the display window, and optionally
// false-color a single channel.
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
    window: vec2<f32>,           // (window_low, gain): displayed = (value - window_low) * gain
    texels_per_pixel: vec2<f32>, // source texels covered by one output pixel
    extent: vec2<f32>,           // image size, in the bound texture's texels
    marks: u32,                  // bit 1: mark pixels at or below black, bit 2: at or above white
    turn: u32,                   // quarter turns clockwise the texture is read through
    primaries: mat3x3<f32>,      // source primaries -> BT.709
    swizzle: u32,                // 0 gray, 1 gray+alpha, 2 rgb, 3 rgba
    alpha_mode: u32,             // 0 opaque, 1 straight, 2 premultiplied
    colormap: u32,               // 0 none, 1 viridis, 2 magma, 3 turbo
    resampler: u32,              // 0 area, 1 antialiased nearest, 2 bicubic
    lift: u32,                   // 0 no gain map, else its channel count; 0 on a coarse level
    _pad2: u32,
    map_size: vec2<f32>,         // the gain map's size, in its own texels
    base_offset: vec4<f32>,      // added to the base before the gain, per channel
    alternate_offset: vec4<f32>, // taken from the product after
    clip: vec4<f32>,             // (x, y, radius, cut) of the circle the quad is cut to; radius 0 for no cut,
                                 // cut 0 for the disc a pixel short of the edge, 1 for the last pixel of it
    picture: vec4<f32>,          // (x, y, width, height) of the image on the target, for a cut quad
};

@group(0) @binding(0) var<uniform> params: Params;
@group(1) @binding(0) var source: texture_2d<f32>;
// The picture's gain map, and the table saying what each of its 256 values
// means at the weight the surface asks for (see image::gain_map). Bound to
// a texel of each, and never read, for a picture with no map.
@group(2) @binding(0) var gain_map: texture_2d<f32>;
@group(2) @binding(1) var gain_table: texture_2d<f32>;

// The marks on clipped pixels, in linear light: a red and a blue that no
// photograph is made of. Twinned by `MARKS` in render/shader_codes.rs, which
// a test holds to these.
const MARK_WHITE: vec3<f32> = vec3<f32>(1.0, 0.02, 0.02);
const MARK_BLACK: vec3<f32> = vec3<f32>(0.02, 0.1, 1.0);

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Where a point of the turned picture, `uv` across the quad, is in the
// texture, which holds the picture as stored: each corner of the quad reads
// the stored corner the turn brought there.
fn turned(uv: vec2<f32>) -> vec2<f32> {
    switch params.turn {
        case 1u: { return vec2<f32>(uv.y, 1.0 - uv.x); }
        case 2u: { return vec2<f32>(1.0 - uv.x, 1.0 - uv.y); }
        case 3u: { return vec2<f32>(1.0 - uv.y, uv.x); }
        default: { return uv; }
    }
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    let uv = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    var out: VertexOut;
    // The quad is the turned picture; the texture is the picture as stored.
    // A quarter turn is linear in uv, so the interpolation between the
    // turned corners is exact and nothing downstream knows the picture was
    // turned.
    out.uv = turned(uv);
    out.position = vec4<f32>(
        params.offset.x + uv.x * params.scale.x,
        params.offset.y - uv.y * params.scale.y,
        0.0,
        1.0,
    );
    return out;
}

// Exact area average: every source texel under the output pixel, each weighted
// by how much of it the pixel actually covers. The count of taps is bounded by
// the coarse chain, which keeps `texels_per_pixel` at four or under however far
// the view zooms out; the clamp is a backstop, not a working limit.
fn area(uv: vec2<f32>) -> vec4<f32> {
    let half = min(params.texels_per_pixel, vec2<f32>(64.0)) * 0.5;
    let center = uv * params.extent;
    let low = center - half;
    let high = center + half;

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

// Nearest neighbor, except across the one output pixel that straddles a texel
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
// so texel centers come through untouched, and sharper than bilinear at the
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

// The false-color ramps, one row per map, as `image_layer` wrote them from
// `Colormap::color`: the readout in the bottom bar names a color from the
// same function, so the swatch and the screen cannot disagree. Which row a
// map is, is `shader_codes::colormap`'s to say.
@group(3) @binding(0) var ramps: texture_2d<f32>;

// The color ramp `which` gives a windowed value, in linear light: the two
// entries either side of it, blended by where it falls between them.
// Out-of-window values take the end of the ramp.
fn false_color(which: u32, t: f32) -> vec3<f32> {
    let last = i32(textureDimensions(ramps).x) - 1;
    let at = clamp(t, 0.0, 1.0) * f32(last);
    let low = i32(floor(at));
    let high = min(low + 1, last);
    let row = i32(which);
    let below = textureLoad(ramps, vec2<i32>(low, row), 0).rgb;
    let above = textureLoad(ramps, vec2<i32>(high, row), 0).rgb;
    return mix(below, above, at - f32(low));
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    var uv = in.uv;
    // The loupe's glass: the quad is the circle's square, cut to the circle,
    // and the image's own place on the target says where in the picture
    // each pixel of it reads. Two quads share the circle. The disc, to a
    // pixel short of the edge, is drawn replacing what is under it, so past
    // the picture's edge it writes nothing, which is the backdrop and not
    // the view underneath. The band, the last pixel, is drawn blending and
    // feathered over the one pixel the edge crosses, so the edge is the
    // glass blended over the view: blended over the backdrop, as the disc
    // would leave it, it would be a hairline of the backdrop's color.
    var coverage = 1.0;
    if params.clip.z > 0.0 {
        let distance = distance(in.position.xy, params.clip.xy);
        let inner = params.clip.z - 1.0;
        if params.clip.w == 0.0 {
            if distance > inner {
                discard;
            }
        } else {
            if distance <= inner {
                discard;
            }
            coverage = clamp(params.clip.z + 0.5 - distance, 0.0, 1.0);
            if coverage <= 0.0 {
                discard;
            }
        }
        let across = (in.position.xy - params.picture.xy) / params.picture.zw;
        if any(across < vec2<f32>(0.0)) || any(across >= vec2<f32>(1.0)) {
            return vec4<f32>(0.0);
        }
        uv = turned(across);
    }
    return shade(uv) * coverage;
}

// The color of the picture at `uv` of the texture: resampled, expanded to
// RGBA, brought into the working space, windowed, marked and false-colored,
// premultiplied by its coverage.
fn shade(uv: vec2<f32>) -> vec4<f32> {
    let texel = resample(uv);

    // Expand whatever we uploaded to RGBA. Gray replicates; alpha defaults
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
    // the primaries matrix act on the actual color rather than a faded one.
    // Bicubic's negative lobes can undershoot, so a texel that has resolved to
    // near-nothing is taken as nothing rather than divided into a wild color.
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

    // While the key for it is held, a pixel the window has taken to white
    // or to black in every channel — where the picture has gone flat, and
    // whatever was there is gone — is painted in a color that is not in any
    // picture, in place of itself: red for the highlights, blue for the
    // shadows, which is what every editor's warning looks like. Before the
    // false color, since a ramp's ends are not white and black, and read
    // in linear light, the same units the corners of the histogram count
    // in. Which ends are marked is `shader_codes::marks`' to say: white is
    // only marked where the surface is actually clipping it.
    if (params.marks & 2u) != 0u && all(windowed >= vec3<f32>(1.0)) {
        return vec4<f32>(MARK_WHITE * alpha, alpha);
    }
    if (params.marks & 1u) != 0u && all(windowed <= vec3<f32>(0.0)) {
        return vec4<f32>(MARK_BLACK * alpha, alpha);
    }

    var result: vec3<f32>;
    if is_gray && params.colormap != 0u {
        result = false_color(params.colormap, windowed.r);
    } else {
        result = windowed;
    }

    // The target blends with premultiplied alpha.
    return vec4<f32>(result * alpha, alpha);
}
