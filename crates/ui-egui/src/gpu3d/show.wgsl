// The frame shown in the window, with what the window's resolution adds over it: edges where
// the surface creases or ends, ambient occlusion and the outline of what is selected or under the
// pointer, each read from gbuffer.rs's targets and each switched by `flags`. Original shader for
// EMCraft (MIT OR Apache-2.0).

struct Show {
    // the viewport's size (pixels), samples per pixel along a side, 1 to encode for an sRGB target
    size: vec2<f32>,
    samples: f32,
    srgb: f32,
    // 1 edges, 2 ambient occlusion, 4 outline; how many tags are selected (in `sel`), the
    // hovered tag (0 none); 1 for a perspective projection
    flags: u32,
    selected: u32,
    hover: u32,
    persp: f32,
    // the window depth's terms: perspective d = a + b / z, orthographic d = a (z - b)
    depth: vec4<f32>,
    outline: vec4<f32>,
    hover_color: vec4<f32>,
    // occlusion: radius (pixels), reach (view depth units), strength
    ao: vec4<f32>,
    // the selected tags (the first `selected`)
    sel: array<vec4<u32>, 2>,
};

@group(0) @binding(0) var frame_t: texture_2d<f32>;
@group(0) @binding(1) var<uniform> show: Show;
@group(0) @binding(2) var normal_t: texture_2d<f32>;
@group(0) @binding(3) var depth_t: texture_2d<f32>;
@group(0) @binding(4) var id_t: texture_2d<u32>;

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn clamped(p: vec2<i32>) -> vec2<i32> {
    let dims = vec2<i32>(textureDimensions(normal_t));
    return clamp(p, vec2<i32>(0), dims - vec2<i32>(1));
}

fn surface(p: vec2<i32>) -> bool {
    return textureLoad(normal_t, clamped(p), 0).w > 0.5;
}

fn depth_at(p: vec2<i32>) -> f32 {
    return textureLoad(depth_t, clamped(p), 0).r;
}

// The view depth from the window depth.
fn view_z(d: f32) -> f32 {
    if (show.persp > 0.5) {
        return show.depth.y / min(d - show.depth.x, -1e-9);
    }
    return d / show.depth.x + show.depth.y;
}

// 1 where the surface ends against the ground, its depth jumps, or it creases.
fn edge(p: vec2<i32>) -> f32 {
    let here = surface(p);
    let nc = textureLoad(normal_t, clamped(p), 0).xyz;
    let dc = depth_at(p);
    var e = 0.0;
    let steps = array<vec2<i32>, 2>(vec2<i32>(1, 0), vec2<i32>(0, 1));
    for (var k = 0; k < 2; k++) {
        let s = steps[k];
        let a = p - s;
        let b = p + s;
        if (surface(a) != here || surface(b) != here) {
            if (here) {
                e = 1.0;
            }
            continue;
        }
        if (!here) {
            continue;
        }
        let da = depth_at(a);
        let db = depth_at(b);
        // A plane's window depth is linear across the screen: its second difference vanishes.
        let bend = abs(da + db - 2.0 * dc);
        if (bend > 0.5 * (abs(da - dc) + abs(db - dc)) + 2e-6) {
            e = 1.0;
        }
        let na = textureLoad(normal_t, clamped(a), 0).xyz;
        let nb = textureLoad(normal_t, clamped(b), 0).xyz;
        if (dot(nc, na) < 0.9 || dot(nc, nb) < 0.9) {
            e = max(e, 0.8);
        }
    }
    return e;
}

// How much nearer surfaces round the pixel hide it: twelve samples on a spiral, each compared
// with the pixel's own plane continued there (its window depth is linear across the screen), so
// a flat face seen at a slant does not hide itself.
fn occlusion(p: vec2<i32>) -> f32 {
    if (!surface(p)) {
        return 0.0;
    }
    let dc = depth_at(p);
    let gx = 0.5 * (depth_at(p + vec2<i32>(1, 0)) - depth_at(p - vec2<i32>(1, 0)));
    let gy = 0.5 * (depth_at(p + vec2<i32>(0, 1)) - depth_at(p - vec2<i32>(0, 1)));
    var occ = 0.0;
    for (var k = 0; k < 12; k++) {
        let a = f32(k) * 2.39996;
        let r = show.ao.x * (f32(k) + 1.0) / 12.0;
        let o = vec2<i32>(round(vec2<f32>(cos(a), sin(a)) * r));
        let q = p + o;
        if (!surface(q)) {
            continue;
        }
        let plane = dc + gx * f32(o.x) + gy * f32(o.y);
        let dz = view_z(plane) - view_z(depth_at(q));
        if (dz > 0.02 * show.ao.y && dz < show.ao.y) {
            occ += 1.0 - dz / show.ao.y;
        }
    }
    return clamp(occ / 12.0 * show.ao.z, 0.0, 0.6);
}

// Whether `id` is `tag` (the hovered), or one of the selected (`tag` 0).
fn chosen(id: u32, tag: u32) -> bool {
    if (id == 0u) {
        return false;
    }
    if (tag != 0u) {
        return id == tag;
    }
    for (var k = 0u; k < min(show.selected, 8u); k++) {
        if (show.sel[k / 4u][k % 4u] == id) {
            return true;
        }
    }
    return false;
}

// 1 on the ring just outside the triangles tagged `tag` (0: those selected).
fn ring(p: vec2<i32>, tag: u32) -> f32 {
    if (chosen(textureLoad(id_t, clamped(p), 0).r, tag)) {
        return 0.0;
    }
    for (var k = 0; k < 8; k++) {
        let a = f32(k) * 0.785398;
        let q = p + vec2<i32>(round(vec2<f32>(cos(a), sin(a)) * 2.0));
        if (chosen(textureLoad(id_t, clamped(q), 0).r, tag)) {
            return 1.0;
        }
    }
    return 0.0;
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
    if ((show.flags & 2u) != 0u) {
        c = c * (1.0 - occlusion(px));
    }
    if ((show.flags & 1u) != 0u) {
        c = mix(c, c * 0.3, 0.75 * edge(px));
    }
    if ((show.flags & 4u) != 0u) {
        if (show.hover != 0u) {
            c = mix(c, show.hover_color.rgb, show.hover_color.a * ring(px, show.hover));
        }
        if (show.selected > 0u) {
            c = mix(c, show.outline.rgb, show.outline.a * ring(px, 0u));
        }
    }
    if (show.srgb > 0.5) {
        c = to_linear(c);
    }
    return vec4<f32>(c, 1.0);
}
