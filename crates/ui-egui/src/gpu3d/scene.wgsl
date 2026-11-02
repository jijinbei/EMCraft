// EMCraft's 3D view on the GPU: the scene of emcraft_render::view3d,
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
    // Origin-relative section plane: dot(normal, world) <= offset. Zero normal disables it.
    clip: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: U;

// The field's plane (field.rs): its frame from the scene's origin (u = (p − origin) · ua), the
// grid (nx, ny, samples per cell, component: 0–2 or 3 for the length), the phase (cos, sin),
// 1 / the colour scale, 1 for a signed map; the map's five stops.
struct F {
    origin: vec4<f32>,
    ua: vec4<f32>,
    va: vec4<f32>,
    grid: vec4<f32>,
    phase: vec4<f32>,
    stops: array<vec4<f32>, 5>,
};

@group(1) @binding(0) var<uniform> f: F;
@group(1) @binding(1) var re_t: texture_2d<f32>;
@group(1) @binding(2) var im_t: texture_2d<f32>;

// The value at sample (i, j) at the phase, and whether it lies inside the mesh.
fn field_value(i: i32, j: i32) -> vec2<f32> {
    let re = textureLoad(re_t, vec2<i32>(i, j), 0);
    let im = textureLoad(im_t, vec2<i32>(i, j), 0).xyz;
    let v = re.xyz * f.phase.x - im * f.phase.y;
    var x = length(v);
    let axis = i32(f.grid.w);
    if (axis == 0) {
        x = v.x;
    } else if (axis == 1) {
        x = v.y;
    } else if (axis == 2) {
        x = v.z;
    }
    return vec2<f32>(x, re.w);
}

// The colour of the field's cell at `p` (as scene3d::field paints it: the mean of the cell's
// corners, the cell left out with a corner outside the mesh: alpha 0).
fn field_colour(p: vec3<f32>) -> vec4<f32> {
    let d = p - f.origin.xyz;
    let n = vec2<f32>(f.grid.x, f.grid.y) - vec2<f32>(1.0);
    let x = clamp(vec2<f32>(dot(d, f.ua.xyz), dot(d, f.va.xyz)), vec2<f32>(0.0), vec2<f32>(1.0)) * n;
    let step = max(f.grid.z, 1.0);
    let c0 = floor(min(x, n - vec2<f32>(1e-3)) / step) * step;
    let c1 = min(c0 + vec2<f32>(step), n);
    let a = field_value(i32(c0.x), i32(c0.y));
    let b = field_value(i32(c1.x), i32(c0.y));
    let c = field_value(i32(c1.x), i32(c1.y));
    let e = field_value(i32(c0.x), i32(c1.y));
    if (min(min(a.y, b.y), min(c.y, e.y)) < 0.5) {
        return vec4<f32>(0.0);
    }
    let mean = 0.25 * (a.x + b.x + c.x + e.x) * f.phase.z;
    var t = mean;
    if (f.phase.w > 0.5) {
        t = 0.5 * (mean + 1.0);
    }
    let s = clamp(t, 0.0, 1.0) * 4.0;
    let k = min(i32(floor(s)), 3);
    return vec4<f32>(mix(f.stops[k].rgb, f.stops[k + 1].rgb, s - f32(k)), 1.0);
}

fn removed(p: vec3<f32>) -> bool {
    return dot(u.clip.xyz, u.clip.xyz) > 0.5 && dot(u.clip.xyz, p) >= u.clip.w - u.forward_nudge.w * 1e-5;
}

// A cylindrical shaft and conical head, twenty-four radial segments. Rust uploads only the
// endpoints, colour and vertex ordinal; geometry and smooth surface normals live here.
struct Glyph {
    p: vec3<f32>,
    n: vec3<f32>,
};

fn arrow_vertex(start: vec3<f32>, end: vec3<f32>, code: f32) -> Glyph {
    let ordinal = u32(max(-code - select(2.0, 1002.0, code < -1000.0), 0.0));
    let segment = ordinal / 15u;
    let k = ordinal % 15u;
    let len = length(end - start);
    let axis = (end - start) / max(len, 1e-12);
    var seed = vec3<f32>(1.0, 0.0, 0.0);
    if (abs(axis.x) > 0.8) { seed = vec3<f32>(0.0, 1.0, 0.0); }
    let ua = normalize(cross(axis, seed));
    let va = cross(axis, ua);
    var next = k == 2u || k == 4u || k == 5u || k == 8u || k == 10u || k == 13u;
    let sides = select(24.0, 8.0, code < -1000.0);
    let angle = 6.28318530718 * f32(segment + select(0u, 1u, next)) / sides;
    let radial = ua * cos(angle) + va * sin(angle);
    var o: Glyph;
    if (k < 6u) {
        let high = k == 1u || k == 2u || k == 4u;
        o.p = start + axis * select(0.0, 0.72 * len, high) + radial * (0.045 * len);
        o.n = radial;
    } else if (k < 9u) {
        o.p = start + axis * (0.72 * len) + radial * (0.12 * len);
        if (k == 7u) { o.p = end; }
        o.n = normalize(radial + axis * (0.12 / 0.28));
    } else {
        let head = k >= 12u;
        let centre = k == 9u || k == 12u;
        o.p = start + axis * select(0.0, 0.72 * len, head);
        if (!centre) { o.p += radial * len * select(0.045, 0.12, head); }
        o.n = -axis;
    }
    return o;
}

fn is_field(flag: f32) -> bool { return flag > -1.5 && flag < -0.5; }

struct V {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    // From the scene's origin; the field's plane when `flag` is −1.
    @location(1) world: vec3<f32>,
    @location(2) flag: f32,
    @location(3) normal: vec3<f32>,
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
    o.pos = placed(p, max(n.w, 0.0));
    o.color = c;
    o.world = p;
    o.normal = n.xyz;
    o.flag = n.w;
    if (is_field(n.w)) {
        o.color = vec4<f32>(1.0, 1.0, 1.0, c.a);
    } else if (n.w < -1.5) {
        let g = arrow_vertex(p, n.xyz, n.w);
        o.pos = placed(g.p, 0.0);
        o.world = g.p;
        o.normal = g.n;
        o.flag = select(-2.0, -3.0, n.w < -1000.0);
    }
    return o;
}

// Per-fragment lighting preserves the cylinder/cone's curved appearance while orbiting.
fn lit(i: V) -> vec4<f32> {
    let n = normalize(i.normal);
    let k = 0.42 + 0.58 * abs(dot(n, u.light.xyz));
    var color = i.color.rgb * k;
    if (i.flag < -1.5) {
        let specular = pow(max(dot(reflect(-u.light.xyz, n), -u.forward_nudge.xyz), 0.0), 24.0);
        color += vec3<f32>(0.25 * specular);
    }
    return vec4<f32>(min(color, vec3<f32>(1.0)), i.color.a);
}

@fragment
fn fs_opaque(i: V) -> @location(0) vec4<f32> {
    if (i.flag != -2.0 && removed(i.world)) { discard; }
    if (is_field(i.flag)) {
        let fc = field_colour(i.world);
        if (fc.a < 0.5) {
            discard;
        }
        return vec4<f32>(fc.rgb * lit(i).rgb, 1.0);
    }
    return vec4<f32>(lit(i).rgb, 1.0);
}

// See-through triangles blended in order (where the device has no order-independent targets).
@fragment
fn fs_blend(i: V) -> @location(0) vec4<f32> {
    if (i.flag != -2.0 && removed(i.world)) { discard; }
    return lit(i);
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
    if (i.flag != -2.0 && removed(i.world)) { discard; }
    let color = lit(i);
    let a = color.a;
    let w = a * clamp(3e3 * pow(1.0 - i.pos.z, 3.0), 1e-2, 3e3);
    var o: Oit;
    o.accum = vec4<f32>(color.rgb * a, a) * w;
    o.reveal = vec4<f32>(a, 0.0, 0.0, 0.0);
    return o;
}

// Depth (and the stencil) only.
@fragment
fn fs_none(i: V) -> @location(0) vec4<f32> {
    if (i.flag != -2.0 && removed(i.world)) { discard; }
    return vec4<f32>(0.0);
}

// Lines: each segment a quad `viewport.z` pixels either side of it, cut at the near plane, both
// ends moved towards the eye so that edges lying on faces pass the depth test.
@vertex
fn vs_line(@builtin(vertex_index) vi: u32, @location(0) a: vec3<f32>, @location(1) b: vec3<f32>, @location(2) c: vec4<f32>) -> V {
    var o: V;
    o.color = c;
    o.world = vec3<f32>(0.0);
    o.flag = 0.0;
    o.normal = vec3<f32>(0.0, 0.0, 1.0);
    // Clip the original world-space segment before expanding it into a screen-space quad.
    var wa = a;
    var wb = b;
    let da = dot(u.clip.xyz, wa) - u.clip.w;
    let db = dot(u.clip.xyz, wb) - u.clip.w;
    if (da > 0.0 && db > 0.0) {
        o.pos = vec4<f32>(0.0, 0.0, -2.0, 1.0);
        return o;
    }
    if (da > 0.0) { wa = mix(wa, wb, da / (da - db)); }
    else if (db > 0.0) { wb = mix(wb, wa, db / (db - da)); }
    let toward = u.forward_nudge.xyz * u.forward_nudge.w;
    var pa = u.view_proj * vec4<f32>(wa - toward, 1.0);
    var pb = u.view_proj * vec4<f32>(wb - toward, 1.0);
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
    @location(3) world: vec3<f32>,
    @location(4) flag: f32,
};

// The field's plane has no triangles where its cells are left out: neither has it pixels here.
fn hole(i: G) -> bool {
    return (i.flag != -2.0 && removed(i.world)) || (is_field(i.flag) && field_colour(i.world).a < 0.5);
}

@vertex
fn vs_g(@location(0) p: vec3<f32>, @location(1) n: vec4<f32>, @location(2) c: vec4<f32>, @location(3) tag: u32) -> G {
    var o: G;
    o.pos = placed(p, max(n.w, 0.0));
    o.n = n.xyz;
    o.tag = tag;
    o.alpha = c.a;
    o.world = p;
    o.flag = n.w;
    if (n.w < -1.5) {
        let g = arrow_vertex(p, n.xyz, n.w);
        o.pos = placed(g.p, 0.0);
        o.n = g.n;
        o.world = g.p;
        o.flag = select(-2.0, -3.0, n.w < -1000.0);
    }
    return o;
}

struct Normals {
    @location(0) n: vec4<f32>,
    // the window depth's bits
    @location(1) depth: vec4<u32>,
};

@fragment
fn fs_g_normals(i: G) -> Normals {
    if (hole(i)) {
        discard;
    }
    var n = normalize(i.n);
    if (dot(n, u.forward_nudge.xyz) > 0.0) {
        n = -n;
    }
    var o: Normals;
    o.n = vec4<f32>(n, select(1.0, 2.0, i.flag < -1.5));
    o.depth = vec4<u32>(bitcast<u32>(i.pos.z), 0u, 0u, 0u);
    return o;
}

@fragment
fn fs_g_any(i: G) -> @location(0) vec4<u32> {
    if (i.tag == 0u || hole(i)) {
        discard;
    }
    return vec4<u32>(i.tag, 0u, 0u, 0u);
}

// Solid: an opacity of 200 or more (the CPU's picking counts them so).
@fragment
fn fs_g_solid(i: G) -> @location(0) vec4<u32> {
    if (i.tag == 0u || i.alpha * 255.0 < 199.5 || hole(i)) {
        discard;
    }
    return vec4<u32>(i.tag, 0u, 0u, 0u);
}
