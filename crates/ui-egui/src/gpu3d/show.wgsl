// The frame shown in the window. Original shader for EMCraft (MIT OR Apache-2.0).

struct Show {
    // the viewport's size (pixels), samples per pixel along a side, 1 to encode for an sRGB target
    size: vec2<f32>,
    samples: f32,
    srgb: f32,
};

@group(0) @binding(0) var frame_t: texture_2d<f32>;
@group(0) @binding(1) var<uniform> show: Show;

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// Each window pixel the mean of its samples × samples block (a box filter, as the CPU's).
@fragment
fn fs_show(i: Out) -> @location(0) vec4<f32> {
    let s = max(i32(show.samples), 1);
    let dims = vec2<i32>(textureDimensions(frame_t));
    let px = vec2<i32>(floor(i.uv * show.size));
    var acc = vec3<f32>(0.0);
    for (var y = 0; y < s; y++) {
        for (var x = 0; x < s; x++) {
            let q = clamp(px * s + vec2<i32>(x, y), vec2<i32>(0), dims - vec2<i32>(1));
            acc += textureLoad(frame_t, q, 0).rgb;
        }
    }
    var c = acc / f32(s * s);
    if (show.srgb > 0.5) {
        c = to_linear(c);
    }
    return vec4<f32>(c, 1.0);
}
