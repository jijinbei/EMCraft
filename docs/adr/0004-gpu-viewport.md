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

## Licences

eframe's `wgpu` feature brings in no crate under a licence other than MIT, Apache-2.0, BSD,
Zlib or their combinations. The crates new to the desktop app's tree:

| Crate | Licence | Why |
|---|---|---|
| `wgpu`, `wgpu-core`, `wgpu-hal`, `wgpu-types`, `naga` | MIT OR Apache-2.0 | the GPU API, its backends and the WGSL compiler |
| `egui-wgpu` | MIT OR Apache-2.0 | egui's wgpu renderer and paint callbacks |
| `ash` | MIT OR Apache-2.0 | Vulkan bindings (Linux, Windows) |
| `gpu-allocator`, `gpu-descriptor`, `gpu-alloc` | MIT OR Apache-2.0 | device memory and descriptors |
| `windows` (DirectX 12 headers), `range-alloc`, `hassle-rs`-free DXC loading | MIT OR Apache-2.0 | DirectX 12 (Windows) |
| `khronos-egl`, `glow`, `glutin_wgl_sys` | MIT OR Apache-2.0 / Apache-2.0 | OpenGL ES backend (already in the tree for glow) |
| `codespan-reporting` | Apache-2.0 | naga's error messages |
| `spirv`, `rspirv`-free `spirv` headers | Apache-2.0 | naga's SPIR-V output for Vulkan |
| `pollster` | Apache-2.0 OR MIT | egui-wgpu waits for the adapter |
| `bit-set`, `bit-vec`, `hexf-parse`, `unicode-xid`, `indexmap`, `arrayvec`, `profiling`, `document-features`, `litrs`, `cfg_aliases`, `raw-window-metal`, `objc2-*` | MIT and/or Apache-2.0 (`hexf-parse` CC0-1.0) | their dependencies |

The full list is checked with `cargo tree -p emcraft -e normal` against the tree before this
change; `cargo deny`-style licence checks are by reading each crate's manifest.

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
