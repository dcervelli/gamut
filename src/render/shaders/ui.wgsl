// Instanced rounded rectangles for the UI layer.
//
// One draw call for the whole interface, however many panels it grows. The UI
// renders into its own sRGB target and never learns what the output surface
// is doing, so widget code can go on thinking in plain sRGB colours while the
// image beside it is in extended-range linear.
//
// An instance is a rounded rectangle about its own centre, turned to face
// `axis`, and either filled or drawn as a band straddling its own outline.
// That covers everything the interface is made of: a panel is a filled box, a
// rule a very thin one, a stroked circle a band on a square rounded as far as
// it will go, and an icon's stroke a band on a box with no height at all —
// which comes out a stadium, round caps and all. Rounding and band are read
// off one signed distance, so every one of them is anti-aliased by the same
// half pixel of feathering, at any angle. Nothing here needs multisampling.

struct Viewport {
    size: vec2<f32>,   // physical pixels
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> viewport: Viewport;

struct Instance {
    // Centre, then half extent along the instance's own two axes, in
    // physical pixels.
    @location(0) bounds: vec4<f32>,
    @location(1) color: vec4<f32>,     // linear, straight alpha
    // The unit vector the instance's own x axis runs along. Everything that
    // lies with the window passes (1, 0).
    @location(2) axis: vec2<f32>,
    @location(3) corner: f32,
    // Zero fills the shape. Anything more draws a band that wide centred on
    // its outline, and leaves the inside alone.
    @location(4) stroke: f32,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Position within the rectangle, in pixels, relative to its centre and
    // along its own axes.
    @location(1) local: vec2<f32>,
    @location(2) half_size: vec2<f32>,
    @location(3) corner: f32,
    @location(4) stroke: f32,
};

// Room left around an instance for the feathering below, in physical pixels.
// A band reaches half its width past the outline it is drawn on, and the
// feather half a pixel past that; a turned rectangle's corners reach past the
// extent an unturned one would have needed.
const PAD: f32 = 1.0;

@vertex
fn vs_main(@builtin(vertex_index) index: u32, instance: Instance) -> VertexOut {
    let centre = instance.bounds.xy;
    let half_size = instance.bounds.zw;
    let reach = half_size + vec2<f32>(instance.stroke * 0.5 + PAD);

    let unit = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    let local = (unit - vec2<f32>(0.5)) * 2.0 * reach;
    let across = vec2<f32>(-instance.axis.y, instance.axis.x);
    let pixel = centre + local.x * instance.axis + local.y * across;

    var out: VertexOut;
    out.position = vec4<f32>(
        pixel.x / viewport.size.x * 2.0 - 1.0,
        1.0 - pixel.y / viewport.size.y * 2.0,
        0.0,
        1.0,
    );
    out.color = instance.color;
    out.local = local;
    out.half_size = half_size;
    out.corner = min(instance.corner, min(half_size.x, half_size.y));
    out.stroke = instance.stroke;
    return out;
}

/// Signed distance to a rounded box, negative inside.
fn rounded_box(point: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(point) - half_size + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let outline = rounded_box(in.local, in.half_size, in.corner);
    // A band on the outline, or everything inside it.
    let distance = select(outline, abs(outline) - in.stroke * 0.5, in.stroke > 0.0);
    // One pixel of feathering, so corners are not stair-stepped.
    let coverage = 1.0 - smoothstep(-0.5, 0.5, distance);
    if coverage <= 0.0 {
        discard;
    }
    // Premultiplied, so that a blend mode which weights the source by the
    // destination — screen — still respects the feathering above.
    let alpha = in.color.a * coverage;
    return vec4<f32>(in.color.rgb * alpha, alpha);
}

// Triangulated fills: arbitrary geometry, one vertex at a time, for plots
// whose shape is not a rectangle. No rounding and no feathering — the outline
// is whatever the caller triangulated.

struct PolyVertex {
    @location(0) position: vec2<f32>,   // physical pixels
    @location(1) color: vec4<f32>,      // linear, straight alpha
};

struct PolyOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_poly(vertex: PolyVertex) -> PolyOut {
    var out: PolyOut;
    out.position = vec4<f32>(
        vertex.position.x / viewport.size.x * 2.0 - 1.0,
        1.0 - vertex.position.y / viewport.size.y * 2.0,
        0.0,
        1.0,
    );
    out.color = vertex.color;
    return out;
}

@fragment
fn fs_poly(in: PolyOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color.rgb * in.color.a, in.color.a);
}
