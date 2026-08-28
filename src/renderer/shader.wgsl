// One axis-aligned quad, positioned in normalised device coordinates by a
// uniform. `offset` is the top-left corner, `scale` the size, with y already
// flipped so that callers can think in top-left-origin window pixels.
struct Quad {
    offset: vec2<f32>,
    scale: vec2<f32>,
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> quad: Quad;

@group(1) @binding(0) var image_texture: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Drawn as a 4-vertex triangle strip: (0,0) (1,0) (0,1) (1,1).
@vertex
fn vs_quad(@builtin(vertex_index) index: u32) -> VertexOut {
    let uv = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    var out: VertexOut;
    out.uv = uv;
    out.position = vec4<f32>(
        quad.offset.x + uv.x * quad.scale.x,
        quad.offset.y - uv.y * quad.scale.y,
        0.0,
        1.0,
    );
    return out;
}

@fragment
fn fs_image(in: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(image_texture, image_sampler, in.uv);
}

@fragment
fn fs_solid(in: VertexOut) -> @location(0) vec4<f32> {
    return quad.color;
}
