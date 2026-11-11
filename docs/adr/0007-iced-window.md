# ADR 0007: A second window in iced, on the rules every window shares

Status: accepted (2026-11-12).

## Context

The egui window draws and decides in the same code: a frame's widgets read the session, keep
their own state and push actions in one pass, so a view of any size (the system canvas was 173
lines in one function) is hard to read. A retained, message-driven toolkit separates the three
— the state, the messages that change it, the view drawn from it — the way docs/style.md asks
code to be written. iced is that toolkit in pure Rust (MIT).

## Decision

- **Shared rules first.** What a window decides — its state and transitions (`UiState`), the
  actions, the dialogs' fields, the views and their handles, the translations, a system's
  layout, gestures, cards and budget — lives in `emcraft-ui-core` (L6, after the engine), with no
  toolkit below it. Both windows build on it; neither repeats it.
- **iced alongside egui.** `crates/ui-iced` (L7) and `apps/emcraft-iced` grow beside the egui
  window, view by view, until the iced one does as much; the egui window stays the shipped one
  until then. The two never depend on each other (`cargo xtask layers`).
- **iced's main line, pinned.** iced 0.14 is built on wgpu 27; the build has wgpu 30 (ADR 0004).
  iced's main line (0.15-dev) is on wgpu 30, so it is taken from git at a fixed revision
  (`Cargo.toml`, `[workspace.dependencies]`), which also brings iced's own forks of winit and
  cosmic-text at fixed revisions (all MIT or Apache-2.0). The revision moves by a deliberate
  change, checked by the gate, and to a release once one is on our wgpu.
- **Features.** wgpu and tiny-skia (the software renderer the headless tests draw with), the
  canvas, images without codecs (pictures arrive as RGBA from `emcraft-render`), X11 and
  Wayland, the thread-pool executor. `iced_test` (the same revision) only as a dev-dependency.
- **One look.** The theme (`emcraft-ui-core::theme`: the tokens, generic over a toolkit's colour) dresses both windows; `emcraft-ui-iced::look` turns the tokens into iced's styles. The iced window reads the same preferences (language, theme, font).
- **Tests.** The window is driven by its messages (typing, dragging, buttons) and drawn headless
  by `iced_test`'s simulator, which also clicks by text; `EMCRAFT_ICED_SHOTS=DIR` writes the
  drawn view as a PNG to look at.

## Consequences

- The build compiles iced and its forks too (a longer cold build; the gate's cache keeps it).
- The first view is a system's node editor; the rest follow, each moving what it decides into
  `emcraft-ui-core` first when the egui window still holds it.
