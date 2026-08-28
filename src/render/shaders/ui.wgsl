// Instanced rounded rectangles for the UI layer.
//
// One draw call for the whole interface, however many panels it grows. The UI
// renders into its own sRGB target and never learns what the output surface
// is doing, so widget code can go on thinking in plain sRGB colours while the
// image beside it is in extended-range linear.

struct Viewport {
    size: vec2<f32>,   // physical pixels
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> viewport: Viewport;

struct Instance {
    @location(0) rect: vec4<f32>,      // x, y, width, height in physical pixels
    @location(1) color: vec4<f32>,     // linear, straight alpha
    @location(2) corner: f32,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Position within the rectangle, in pixels, relative to its centre.
    @location(1) local: vec2<f32>,
    @location(2) half_size: vec2<f32>,
    @location(3) corner: f32,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32, instance: Instance) -> VertexOut {
    let unit = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    let pixel = instance.rect.xy + unit * instance.rect.zw;
    let half_size = instance.rect.zw * 0.5;

    var out: VertexOut;
    out.position = vec4<f32>(
        pixel.x / viewport.size.x * 2.0 - 1.0,
        1.0 - pixel.y / viewport.size.y * 2.0,
        0.0,
        1.0,
    );
    out.color = instance.color;
    out.local = (unit - vec2<f32>(0.5)) * instance.rect.zw;
    out.half_size = half_size;
    out.corner = min(instance.corner, min(half_size.x, half_size.y));
    return out;
}

/// Signed distance to a rounded box, negative inside.
fn rounded_box(point: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(point) - half_size + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let distance = rounded_box(in.local, in.half_size, in.corner);
    // One pixel of feathering, so corners are not stair-stepped.
    let coverage = 1.0 - smoothstep(-0.5, 0.5, distance);
    if coverage <= 0.0 {
        discard;
    }
    return vec4<f32>(in.color.rgb, in.color.a * coverage);
}
