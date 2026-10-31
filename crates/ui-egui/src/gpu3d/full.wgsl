// EMCraft's 3D view on the GPU: the vertex stage of the full-screen passes, which each fragment
// stage (composite.wgsl, show.wgsl) follows. Original shader for EMCraft (MIT OR Apache-2.0).

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// A full-screen triangle; uv 0–1 across the viewport, (0, 0) at its top left.
@vertex
fn vs(@builtin(vertex_index) i: u32) -> Out {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var o: Out;
    o.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}

