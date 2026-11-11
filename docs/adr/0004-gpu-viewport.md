# ADR 0004: The 3D view drawn only by the GPU, with wgpu

Status: accepted (2026-10-31; GPU-only from 2026-11-01).

## Decision

The 3D view is drawn by the GPU with a depth buffer (`crates/gpu3d`, L7), and only by
it: the CPU 3D rasterizer (`emcraft_render::view3d::render`, `flatten`, the egui-mesh drag path)
is gone. `emcraft_render::view3d` keeps the scene, the camera, the section view and picking by ray
(through its bounding-volume hierarchy), which the tools' snapping and the fallback read. Charts
and field plots stay on the CPU (`emcraft_render`'s 2D renderer).

- **Where it draws.** The desktop window draws with eframe's **wgpu** renderer; the view is drawn
  by the window's device in an `egui_wgpu` paint callback. Without a window device of its own —
  the window opened with eframe's **glow** renderer (OpenGL 2) because wgpu could not, the
  headless harness, `ui.render`, the start page's and the template cards' pictures — the same
  renderer draws on a device of the process's own (`gpu3d::headless`: any adapter, a software one
  such as Mesa's lavapipe or llvmpipe or Windows' WARP when no other is found) and the picture is
  read back into a texture. With no adapter at all the view says so.
- **Versions.** eframe and egui-wgpu 0.36.2, wgpu 30.0.1 (the line egui 0.36 is built on;
  SolidCraft runs wgpu 30 with egui 0.37). No version of egui moves.
- **Why wgpu and not glow.** Every glow call is `unsafe` (raw OpenGL), and the workspace
  forbids `unsafe`. wgpu is safe Rust, validates every call, reports errors as values (error
  scopes, the device's error handler) and runs on Vulkan, DirectX 12, Metal and OpenGL ES 3.
- **No patched egui-wgpu.** SolidCraft vendors egui-wgpu 0.37 to clamp the window's MSAA to
  what the device supports and to match its sample count with the paint callbacks'. EMCraft
  needs neither: the 3D view draws into its own offscreen targets (multisampled, with depth and
  stencil, sized by the view) and the callback only shows the resolved frame in egui's pass,
  which stays one sample with no depth (`NativeOptions { multisampling: 0, depth_buffer: 0 }`).
- **Fallbacks, in order.** (1) wgpu cannot open the window: eframe's glow renderer opens it
  (`run_with`; `EMCRAFT_RENDERER=glow` forces it) and the view draws offscreen; (2) the
  pipelines cannot be made on the window's device, or a GPU error later (out of memory, a lost
  device; the device's error handler, which replaces wgpu's panicking one, marks it): the view
  draws offscreen; (3) no adapter at all: a message in the view; (4) neither renderer opens a
  window: the error and the message box, as before.
- **Tested against known answers**, not against another renderer (`gpu3d::tests`): faces at
  known places and colours read back pixel by pixel against the shading formula (ambient plus
  Lambert from a light above and right of the eye, both sides lit alike) and their depth order;
  one see-through layer blending exactly by its opacity and two covering 1 − (1 − a)²; a line in
  front of glass drawn whole and one behind it blended; nested section caps; the tags under a
  pixel against rays met analytically; the field's colours against the colour map of known
  values; smoothing, edges, occlusion and the outline only where they belong. They run on any
  adapter, a software one included (`EMCRAFT_GPU_FALLBACK=1` asks for it); without any they skip
  unless `EMCRAFT_REQUIRE_GPU` is set. The projection is checked against the camera's without a
  GPU (`gpu3d::pack`), and every WGSL file is validated by naga. Checked on an NVIDIA GPU
  (Vulkan), on Mesa's lavapipe (`VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
  WGPU_BACKEND=vulkan`) and on llvmpipe through OpenGL ES (`WGPU_BACKEND=gl
  LIBGL_ALWAYS_SOFTWARE=1`); the per-pixel depth is kept as the bits of an f32 in an integer
  target, as OpenGL ES cannot render to 32-bit float ones.
- **Section clipping in WGSL (2026-11-02).** The GPU receives the uncut scene and an
  origin-relative clipping plane. Fragment shaders discard the removed half in opaque,
  transparent, depth/stencil, normals and picking passes; the line vertex shader clips each
  world-space segment before screen-space expansion. A CPU cut remains the reference for
  ray picking, snapping and dimension outlines; it is not uploaded as the rendered geometry.
  Mesh-cell perimeters on the cut are derived from the tetrahedral topology in Rust and
  uploaded as lines. No cap fills or metal backing are drawn.
- **Port direction glyphs in WGSL.** Only endpoints and colours are supplied by Rust. The
  vertex shader constructs cylindrical shafts and conical heads, including surface normals;
  the fragment shader lights the curved surfaces and adds a small specular highlight. The
  same construction supplies the normals/depth pass. Glyphs are untagged annotations, kept
  whole across a section and depth-tested against model geometry; they do not intercept picks.
  Waveguide and lumped ports use these glyphs instead of egui arrows. Field-vector arrows use the same procedural geometry and follow the section clip; their sampled vectors still come from the solver in Rust. Labels, rulers and UI
  controls stay in egui. Model topology, parameter validation and interaction stay in Rust.
- **Determinism.** What the GPU draws is a display: it is not a result, not saved and not
  compared bit for bit; the scenarios check the view's state (`ui.inspect`), not its pixels.
- **Settings.** Settings ▸ 3D view ▸ Drawing: the switches below. `ui.inspect`'s
  `view3d.renderer` names what drew the last frame (`gpu` the window's device, `offscreen`, or
  `none`) and `view3d.gpu_adapter` the adapter.

## What the GPU adds (shaders, each switched)

WGSL is the one language besides Rust (AGENTS.md): the viewport's rendering shaders, compiled by
naga at run time; a unit test validates every `.wgsl` file with naga, no GPU needed. Beyond the
plain picture the view draws, each with a switch in Settings ▸ 3D view ▸ Drawing
(`prefs.gpu_look`):

- **Per-pixel targets** at the window's resolution (`gpu3d::gbuffer`): the opaque surface's
  normal and window depth, and the tags of the nearest tagged and the nearest solid triangle.
- **Edges** (on): where the surface ends against the ground, its depth jumps (the second
  difference of the window depth, which vanishes on a plane) or its normal turns past ~25°.
- **Ambient occlusion** (off): twelve samples on a spiral, each compared with the pixel's own
  plane continued there, darkening by up to 60 %; cheap enough for every frame.
- **Outline** (on): a two-pixel ring round what is selected and, fainter, what is under the
  pointer (read from the tags), in the accent colour; the scene's own highlight stays.
- **Picking** (on): a click reads the two tags under the pointer back (one pixel, at once) and
  takes the solid one, else the nearest, as `emcraft_render::view3d::pick` does; untagged
  triangles hide nothing, as for the rays. Only when the frame shown is the one asked about
  (scene, camera and size; `Frame::key`), else the rays through the scene's bounding volumes
  (`emcraft_render::view3d::bvh`); the tools' snapping stays on the rays. Checked against rays
  met analytically.
- **The field's colour map** (on): the plane's phasors (`PlaneSamples::phasors`: Re and Im of E
  or H at each sample) uploaded once; the fragment shader evaluates Re(F e^{jφ}) (a component
  or the length), averages each cell's corners and colours it with the theme's map as
  `scene3d::field` does — the same picture, checked cell by cell. The scene keeps the plane as one
  quad (picking, outline); without arrows the phase leaves the scene's key, so playing the phase
  rebuilds nothing. For a single plane, complex E/H vectors are uploaded with the glyphs once, then rotated as Re(F exp(jφ)) in the vertex shader; scene and vertex buffers stay unchanged during playback. Shaft/head widths have a screen-space minimum and E/H use distinct theme colors without a white halo. With kept planes the arrows still use the CPU phase path; with kept planes the
  scene holds every plane's coloured cells (`scene3d::field`) and the GPU draws them as faces.

## A library instead of our own pipelines?

What the view must do is narrow: draw one list of coloured triangles (opaque and see-through,
each with a tag) and lines, with a depth buffer. The scene, the
section and its caps, the field's colour maps (vertex colours), the highlight and picking (rays
against the scene) are ours already and stay on the CPU, because the tools need them there. So a library would only replace the drawing of that list — about 900
lines here (`gpu3d`: packing, projection, five passes, three small WGSL shaders).

| Option | Licence | Size | What we would still write | Our look (shading, lines on faces) | Windows (MinGW cross build) |
|---|---|---|---|---|---|
| **wgpu offscreen + paint callback (chosen)** | MIT OR Apache-2.0 | +4.6 MB (Linux), +5.7 MB (Windows) | everything above, once; no `unsafe` | ours by construction: the camera's projection, our shading, line width and nudge, checked against known answers | builds (`cargo xtask installer-windows`); DirectX 12, Vulkan, GL |
| glow paint callback with a depth buffer (eframe's current backend) | MIT OR Apache-2.0 | +0 | the same, in GLSL | the same | builds |
| `three-d` 0.19 (glow, egui integration) | MIT | +1–2 MB | the scene's packing, see-through ordering (it blends in draw order, no OIT), wide lines with the nudge (its lines are meshes), our shading as a custom material, caps, colour maps, picking | only after matching its materials to our shading | builds (glow) |
| `truck-platform` / `truck-rendimpl` (wgpu) | Apache-2.0 | wgpu and more | caps, lines, transparency, colour maps, picking; it draws B-rep shapes and polygon meshes with its own lighting | no: its own PBR-like shading | as wgpu; not in our tree (SolidCraft brings only truck's kernel crates) |
| `bevy`, `kiss3d` | MIT OR Apache-2.0 / BSD-3-Clause | tens of MB (bevy) | — | — | ruled out: an engine with its own window and loop (bevy), or its own window (kiss3d); neither lives inside an egui panel |

- **glow is out** because every glow call is `unsafe` and the workspace forbids `unsafe`; with
  wgpu the same renderer is safe Rust and its errors are values.
- **three-d is not taken.** It would not remove the hard parts (our look, see-through
  faces, lines on faces, caps, picking), it ties our glow and egui versions to its own (it must
  use eframe's GL context, so its `glow` must be eframe's), and it is slowing: its latest
  release on crates.io is 0.19.0 (2026-04-17), the last change 2026-06-24 (winit 0.30), one
  maintainer and one or two releases a year — it could hold back an egui upgrade. wgpu is
  maintained by a large group, released every quarter, and is what egui's own wgpu backend and
  SolidCraft use.
- **truck's renderer** draws CAD shapes with its own shading; it shares nothing with our scene
  and is not in our tree.
- **The choice:** our own small renderer on wgpu, the approach SolidCraft (the sibling app)
  takes, so fixes and techniques move between the two.

## Licences

eframe's `wgpu` feature adds these crates to the desktop app's tree (`cargo tree -p emcraft -e
normal` with and without it, for `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-gnu`), each
read from its manifest; no other licence than MIT, Apache-2.0 or both:

| Crate | Licence | Why |
|---|---|---|
| `wgpu`, `wgpu-core`, `wgpu-hal`, `wgpu-types`, `wgpu-naga-bridge`, `wgpu-core-deps-windows-linux-android` 30.0.1 | MIT OR Apache-2.0 | the GPU API and its backends |
| `naga`, `naga-types` 30.0.1 | MIT OR Apache-2.0 | the WGSL compiler (to SPIR-V, HLSL, GLSL) |
| `egui-wgpu` 0.36.2 | MIT OR Apache-2.0 | egui's wgpu renderer and paint callbacks |
| `ash` 0.38 | MIT OR Apache-2.0 | Vulkan |
| `gpu-allocator` 0.28, `presser`, `range-alloc` (Windows) | MIT OR Apache-2.0 | device memory |
| `khronos-egl` 6, `as-raw-xcb-connection` (Linux) | MIT OR Apache-2.0 | OpenGL ES through EGL |
| `windows` 0.62 and its `windows-*` crates (Windows) | MIT OR Apache-2.0 | DirectX 12 |
| `renderdoc-sys` | MIT OR Apache-2.0 | RenderDoc's capture API (loaded only when RenderDoc is) |
| `spirv` 0.4, `codespan-reporting` 0.13 | Apache-2.0 | naga's SPIR-V output, its error messages |
| `ordered-float` 5 | MIT | naga |
| `bit-set`, `bit-vec`, `rustc-hash` 1, `type-map`, `unicode-width`, `pollster` | MIT OR Apache-2.0 | their small helpers |

## Binary size and drivers

- Measured 2026-11-01, `cargo build --release -p emcraft` (stripped, fat LTO): the Linux binary
  grows from 101.9 MB (main, glow only; most of it CubeCL's CUDA runtime and LLVM) to 106.5 MB
  (+4.6 MB: wgpu, naga, the Vulkan and GL backends, the renderer). The Windows build
  (`cargo xtask installer-windows`, MinGW): `emcraft.exe` 42 MB → 47.7 MB (DirectX 12, Vulkan
  and GL), the installer 23.6 MB → 25.3 MB.
- **Linux:** wgpu prefers Vulkan (Mesa's drivers, NVIDIA's, lavapipe in software), then OpenGL
  ES 3 through EGL. A machine with only OpenGL 2 opens the window with glow.
- **Windows:** wgpu prefers DirectX 12 (Windows 10 and later), then Vulkan, then OpenGL through
  WGL. The installer's build is cross-compiled for `x86_64-pc-windows-gnu` with MinGW-w64 and
  builds with the wgpu backends (`cargo xtask installer-windows`). DirectX 12 compiles shaders
  with FXC (`d3dcompiler_47.dll`, part of Windows); WARP is the software adapter where there is
  no GPU. Under Wine with Mesa's lavapipe the window opens on wgpu and draws the view on it
  (docs/install-windows.md); without Vulkan the app falls back to OpenGL as above.
- **macOS:** Metal (not built or shipped today).

Vector glyph vertices carry the center, real and imaginary displacement, maximum length and vertex ordinal (64 bytes per vertex). Both the color and g-buffer stages use the same WGSL phase evaluation. Categorical flags are flat interpolated.

PCB explanatory curves use the same vertex layout with a separate ordinal range. Rust uploads
bounded arch/ellipse/line descriptors and colours; `curve_vertex` constructs the tube surface
and smooth normals in WGSL for both colour and g-buffer passes. These curves have a minimum
screen-space radius, ordinary per-fragment glyph lighting and no white outline. They carry no
field samples or physical phase. Analytical arch/ellipse point checks read back the GPU image;
the PCB click scenario checks the magnified default, orbit and light/dark views.
