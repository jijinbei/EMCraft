// EMCraft's 3D view on the GPU: the scene the CPU renderer draws (emcraft_render::view3d),
// shaded the same way. Original shader for EMCraft (MIT OR Apache-2.0).

struct U {
    view_proj: mat4x4<f32>,
    // forward (xyz), how far lines move towards the eye (w, world units)
    forward_nudge: vec4<f32>,
    light: vec4<f32>,
    // target width, height (pixels), half a line's width (pixels), nearest clip w kept
    viewport: vec4<f32>,
    top: vec4<f32>,
    bottom: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: U;

struct V {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

// The ground: a full-screen triangle, top to bottom by pixel row as the CPU fills it.
@vertex
fn vs_ground(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 1.0, 1.0);
}

@fragment
fn fs_ground(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let f = clamp((pos.y - 0.5) / max(u.viewport.y - 1.0, 1.0), 0.0, 1.0);
    return vec4<f32>(mix(u.top.rgb, u.bottom.rgb, f), 1.0);
}

// Triangles: ambient plus Lambert from a light above and right of the eye, both sides alike.
// `n.w` ranks coplanar faces: each rank moves the face towards the eye by 1/40 of the lines'
// nudge, so the later of two faces in one plane is drawn as on the CPU, not whichever the depth
// buffer's rounding picks; the lines, nudged further than 15 ranks, stay in front.
fn placed(p: vec3<f32>, rank: f32) -> vec4<f32> {
    let toward = u.forward_nudge.xyz * (u.forward_nudge.w * 0.025 * rank);
    return u.view_proj * vec4<f32>(p - toward, 1.0);
}

@vertex
fn vs_tri(@location(0) p: vec3<f32>, @location(1) n: vec4<f32>, @location(2) c: vec4<f32>) -> V {
    var o: V;
    o.pos = placed(p, n.w);
    let k = 0.42 + 0.58 * abs(dot(n.xyz, u.light.xyz));
    o.color = vec4<f32>(c.rgb * k, c.a);
    return o;
}

@fragment
fn fs_opaque(i: V) -> @location(0) vec4<f32> {
    return vec4<f32>(i.color.rgb, 1.0);
}

// See-through triangles blended in order (where the device has no order-independent targets).
@fragment
fn fs_blend(i: V) -> @location(0) vec4<f32> {
    return i.color;
}

// Weighted blended order-independent transparency (McGuire and Bavoil, JCGT 2(2), 2013): the
// premultiplied colour weighted by opacity and depth (their eq. 10, d the window depth), and the
// opacity, whose product of complements the second target gathers.
struct Oit {
    @location(0) accum: vec4<f32>,
    @location(1) reveal: vec4<f32>,
};

@fragment
fn fs_oit(i: V) -> Oit {
    let a = i.color.a;
    let w = a * clamp(3e3 * pow(1.0 - i.pos.z, 3.0), 1e-2, 3e3);
    var o: Oit;
    o.accum = vec4<f32>(i.color.rgb * a, a) * w;
    o.reveal = vec4<f32>(a, 0.0, 0.0, 0.0);
    return o;
}

// Depth (and the stencil) only.
@fragment
fn fs_none(i: V) -> @location(0) vec4<f32> {
    return vec4<f32>(0.0);
}

// Lines: each segment a quad `viewport.z` pixels either side of it, cut at the near plane, both
// ends moved towards the eye so that edges lying on faces pass the depth test.
@vertex
fn vs_line(@builtin(vertex_index) vi: u32, @location(0) a: vec3<f32>, @location(1) b: vec3<f32>, @location(2) c: vec4<f32>) -> V {
    var o: V;
    o.color = c;
    let toward = u.forward_nudge.xyz * u.forward_nudge.w;
    var pa = u.view_proj * vec4<f32>(a - toward, 1.0);
    var pb = u.view_proj * vec4<f32>(b - toward, 1.0);
    let near = u.viewport.w;
    if (pa.w < near && pb.w < near) {
        o.pos = vec4<f32>(0.0, 0.0, -2.0, 1.0);
        return o;
    }
    if (pa.w < near) {
        pa = mix(pa, pb, (near - pa.w) / (pb.w - pa.w));
    } else if (pb.w < near) {
        pb = mix(pb, pa, (near - pb.w) / (pa.w - pb.w));
    }
    let half_size = u.viewport.xy * 0.5;
    let sa = pa.xy / pa.w * half_size;
    let sb = pb.xy / pb.w * half_size;
    let d = sb - sa;
    let len = length(d);
    var dir = vec2<f32>(1.0, 0.0);
    if (len > 1e-6) {
        dir = d / len;
    }
    let side = vec2<f32>(-dir.y, dir.x) * u.viewport.z;
    // Two triangles: (a−, b−, b+), (a−, b+, a+).
    let k = vi % 6u;
    let at_b = k == 1u || k == 2u || k == 4u;
    let plus = k == 2u || k == 4u || k == 5u;
    var p = pa;
    if (at_b) {
        p = pb;
    }
    var s = -1.0;
    if (plus) {
        s = 1.0;
    }
    o.pos = vec4<f32>(p.xy + side * s / half_size * p.w, p.z, p.w);
    return o;
}

@fragment
fn fs_line(i: V) -> @location(0) vec4<f32> {
    return i.color;
}

// What lies under each pixel (gbuffer.rs): the opaque surface's normal (turned to the eye) and
// window depth, and the tags of the nearest tagged and the nearest solid triangle.
struct G {
    @builtin(position) pos: vec4<f32>,
    @location(0) n: vec3<f32>,
    @location(1) @interpolate(flat) tag: u32,
    @location(2) alpha: f32,
};

@vertex
fn vs_g(@location(0) p: vec3<f32>, @location(1) n: vec4<f32>, @location(2) c: vec4<f32>, @location(3) tag: u32) -> G {
    var o: G;
    o.pos = placed(p, n.w);
    o.n = n.xyz;
    o.tag = tag;
    o.alpha = c.a;
    return o;
}

struct Normals {
    @location(0) n: vec4<f32>,
    @location(1) depth: vec4<f32>,
};

@fragment
fn fs_g_normals(i: G) -> Normals {
    var n = normalize(i.n);
    if (dot(n, u.forward_nudge.xyz) > 0.0) {
        n = -n;
    }
    var o: Normals;
    o.n = vec4<f32>(n, 1.0);
    o.depth = vec4<f32>(i.pos.z, 0.0, 0.0, 0.0);
    return o;
}

@fragment
fn fs_g_any(i: G) -> @location(0) vec4<u32> {
    if (i.tag == 0u) {
        discard;
    }
    return vec4<u32>(i.tag, 0u, 0u, 0u);
}

// Solid: an opacity of 200 or more (the CPU's picking counts them so).
@fragment
fn fs_g_solid(i: G) -> @location(0) vec4<u32> {
    if (i.tag == 0u || i.alpha * 255.0 < 199.5) {
        discard;
    }
    return vec4<u32>(i.tag, 0u, 0u, 0u);
}
