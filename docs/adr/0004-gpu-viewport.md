# ADR 0004: The 3D view on the window's GPU with wgpu

Status: accepted (2026-10-31).

## Decision

The desktop window draws with eframe's **wgpu** renderer, and the 3D view is drawn by the
window's GPU with a depth buffer (`crates/ui-egui/src/gpu3d`, L7) every frame, still and moving.
eframe keeps its **glow** renderer as well: where wgpu cannot open the window the app opens it
again with OpenGL 2, and the 3D view is drawn as before (the CPU's picture at rest, egui's
painter while moving). The CPU renderer (`emcraft_render::view3d`) stays the reference: headless
pictures (`ui.render`, the scenarios, the CLI) and every fallback draw with it.

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
  The window's pass never sees a sample count or a depth format of ours.
- **Fallbacks, in order.** (1) wgpu cannot open the window (no adapter, no surface): eframe's
  glow renderer opens it (`run_with`; `EMCRAFT_RENDERER=glow` forces it), the 3D view as before;
  (2) the 3D view's pipelines cannot be made on the window's device: the view draws on the CPU
  (`gpu3d::install` fails, reported on stderr); (3) a GPU error later (out of memory, a lost
  device): the device's error handler, which replaces wgpu's default panicking one, marks the
  view failed and it draws on the CPU from the next frame. (4) Neither renderer opens a window:
  the error and the message box, as before.
- **The same picture.** Both renderers draw one `Scene` (triangles with colour, opacity and tag,
  lines). The GPU's transform is the CPU projector's (the same pixel for the same point, checked
  in `gpu3d::pack`'s tests), its shading the CPU's (ambient plus Lambert from a light above and
  right of the eye, both sides lit alike), lines as wide as the CPU's and moved towards the eye
  by the same distance so edges on faces show. See-through faces use weighted blended
  order-independent transparency (McGuire and Bavoil 2013) where the CPU sorts them: the two
  agree where faces do not overlap and differ a little where they do. A test draws a scene on a
  headless adapter and compares it with the CPU's picture pixel by pixel (it skips without an
  adapter).
- **Section caps without a stencil cap pass.** SolidCraft caps its section with the stencil.
  EMCraft's section is cut on the CPU (`emcraft_render::view3d::section`: the far side removed,
  the cut faces capped), because picking and the tools read the cut scene; the GPU draws that
  scene, so the caps are the CPU's, exactly. The stencil is used instead to draw lines over the
  see-through faces they lie in front of.
- **Tints.** The selection's and the hovered row's highlight are colours in the scene
  (`scene3d`), so both renderers show them; no per-tag tint uniform.
- **Determinism.** What the GPU draws is a display: it is not a result, not saved and not
  compared bit for bit. Pictures that are compared (scenarios, `ui.render`, exports) are the
  CPU's.
- **Settings.** Settings ▸ 3D view ▸ Drawing: GPU (the default; Quality where there is no GPU
  view), Quality (the CPU's picture at rest, egui's painter while moving) or Fast (egui's
  painter always). `ui.inspect`'s `view3d.renderer` names what drew the last frame (`gpu`,
  `cpu`, `painter`).

## A library instead of our own pipelines?

What the view must do is narrow: draw one list of coloured triangles (opaque and see-through,
each with a tag) and lines, as the CPU renderer draws them, with a depth buffer. The scene, the
section and its caps, the field's colour maps (vertex colours), the highlight and picking (rays
against the scene) are ours already and stay on the CPU, because the headless pictures and the
tools need them there. So a library would only replace the drawing of that list — about 900
lines here (`gpu3d`: packing, projection, five passes, three small WGSL shaders).

| Option | Licence | Size | What we would still write | CPU parity | Windows (MinGW cross build) |
|---|---|---|---|---|---|
| **wgpu offscreen + paint callback (chosen)** | MIT OR Apache-2.0 | +7 MB (Linux, stripped) | everything above, once; no `unsafe` | exact by construction: our projection, shading, line width and nudge, checked pixel by pixel | builds (`cargo xtask installer-windows`); DirectX 12, Vulkan, GL |
| glow paint callback with a depth buffer (eframe's current backend) | MIT OR Apache-2.0 | +0 | the same, in GLSL | the same | builds |
| `three-d` 0.19 (glow, egui integration) | MIT | +1–2 MB | the scene's packing, see-through ordering (it blends in draw order, no OIT), wide lines with the nudge (its lines are meshes), our shading as a custom material, caps, colour maps, picking | only after matching its materials to our shading | builds (glow) |
| `truck-platform` / `truck-rendimpl` (wgpu) | Apache-2.0 | wgpu and more | caps, lines, transparency, colour maps, picking; it draws B-rep shapes and polygon meshes with its own lighting | no: its own PBR-like shading | as wgpu; not in our tree (SolidCraft brings only truck's kernel crates) |
| `bevy`, `kiss3d` | MIT OR Apache-2.0 / BSD-3-Clause | tens of MB (bevy) | — | — | ruled out: an engine with its own window and loop (bevy), or its own window (kiss3d); neither lives inside an egui panel |

- **glow is out** because every glow call is `unsafe` and the workspace forbids `unsafe`; with
  wgpu the same renderer is safe Rust and its errors are values.
- **three-d is not taken.** It would not remove the hard parts (the CPU's look, see-through
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

- The Linux release binary grows from 23.1 MB to 30.4 MB (`cargo build --release -p emcraft`,
  stripped, fat LTO); the Windows one similarly (the DirectX 12 and Vulkan backends).
- **Linux:** wgpu prefers Vulkan (Mesa's drivers, NVIDIA's, lavapipe in software), then OpenGL
  ES 3 through EGL. A machine with only OpenGL 2 opens the window with glow.
- **Windows:** wgpu prefers DirectX 12 (Windows 10 and later), then Vulkan, then OpenGL through
  WGL. The installer's build is cross-compiled for `x86_64-pc-windows-gnu` with MinGW-w64 and
  builds with the wgpu backends (`cargo xtask installer-windows`). DirectX 12 compiles shaders
  with FXC (`d3dcompiler_47.dll`, part of Windows). Under Wine without a GPU, wgpu finds no
  adapter and the window opens with glow (or reports, as before, when there is no OpenGL).
- **macOS:** Metal (not built or shipped today).
