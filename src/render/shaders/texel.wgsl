// Reading one texel of the image as uploaded, shared by the image layer and
// the coarse chain: `load` finds it, lifts it where the picture has a gain
// map, and premultiplies it. Prepended to `image.wgsl` and `reduce.wgsl` by
// `render::mod`, so that the two cannot drift; each declares the `params`,
// `source`, `gain_map` and `gain_table` this reads, at the names used here.

// Color premultiplied by alpha, in the component layout the texture stores.
// Every filter that reads it is a weighted sum of texels, and weighting
// straight alpha would drag the color of fully transparent texels into their
// neighbors, which is what shows as haloing along a hard edge. Coarse levels
// are already premultiplied, so this is a no-op for them.
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

// The gains the table gives one value of the map, per channel: for a
// one-channel map the three gains of its one value; for a three-channel
// map each channel's gain of its own value.
fn gains(coord: vec2<i32>) -> vec3<f32> {
    let value = textureLoad(gain_map, coord, 0);
    let at = vec3<i32>(round(value.rgb * 255.0));
    if params.lift == 1u {
        return textureLoad(gain_table, vec2<i32>(at.r, 0), 0).rgb;
    }
    return vec3<f32>(
        textureLoad(gain_table, vec2<i32>(at.r, 0), 0).r,
        textureLoad(gain_table, vec2<i32>(at.g, 0), 0).g,
        textureLoad(gain_table, vec2<i32>(at.b, 0), 0).b,
    );
}

// The gain at texel `coord` of the image as uploaded: the map's four
// surrounding values, each through the table, blended by the texel's
// distance from them. The twin of `GainMap::gain_at` in image/gain_map.rs;
// keep the two in step.
fn gain(coord: vec2<i32>) -> vec3<f32> {
    let position = vec2<f32>(coord) / params.extent * params.map_size;
    let last = vec2<i32>(textureDimensions(gain_map)) - vec2<i32>(1);
    let near = min(vec2<i32>(floor(position)), last);
    let far = min(near + vec2<i32>(1), last);
    let fraction = position - floor(position);
    let top = mix(gains(near), gains(vec2<i32>(far.x, near.y)), fraction.x);
    let bottom = mix(gains(vec2<i32>(near.x, far.y)), gains(far), fraction.x);
    return mix(top, bottom, fraction.y);
}

// One texel, lifted where the picture has a gain map — before it is
// premultiplied, filtered or converted, since the map multiplies light —
// and premultiplied.
fn load(coord: vec2<i32>) -> vec4<f32> {
    let limit = vec2<i32>(textureDimensions(source)) - vec2<i32>(1);
    let clamped = clamp(coord, vec2<i32>(0), limit);
    var texel = textureLoad(source, clamped, 0);
    if params.lift != 0u {
        let lifted = (texel.rgb + params.base_offset.rgb) * gain(clamped) - params.alternate_offset.rgb;
        texel = vec4<f32>(lifted, texel.a);
    }
    return premultiplied(texel);
}

