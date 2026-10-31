// The see-through layer laid over the frame. Original shader for EMCraft (MIT OR Apache-2.0).

@group(0) @binding(0) var accum_t: texture_2d<f32>;
@group(0) @binding(1) var reveal_t: texture_2d<f32>;

// The weighted average of the see-through colours, covering by one minus the revealage
// (McGuire and Bavoil 2013), blended over the frame.
@fragment
fn fs_composite(i: Out) -> @location(0) vec4<f32> {
    let p = vec2<i32>(i.pos.xy);
    let reveal = textureLoad(reveal_t, p, 0).r;
    if (reveal >= 1.0) {
        discard;
    }
    let accum = textureLoad(accum_t, p, 0);
    return vec4<f32>(accum.rgb / clamp(accum.a, 1e-5, 5e4), 1.0 - reveal);
}

