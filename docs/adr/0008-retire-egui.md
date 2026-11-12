# ADR 0008: Retiring the egui window

Status: accepted (2026-11-12); stages A and B done.

## Context

ADR 0007 grew an iced window beside the egui one on the rules both share
(`emcraft-ui-core`). The iced window now has every page, the 3D view's tools, grips, navigator
and section, the control channel and its scenarios; the egui one still does more (about 22 000
lines against 7 000). Keeping two windows doubles every UI change.

## Decision

- **Port what the egui window does alone, then switch at once.** Each stage writes the rules
  into `emcraft-ui-core`, builds the iced view on them, and writes the iced scenarios that
  replace the egui ones for it. From stage B on the egui window is left as it is (stage A also
  moved its workflow onto the shared rules): it keeps running and its tests keep passing until
  it goes, and `cargo xtask dupes` leaves `crates/ui-egui` out meanwhile, since its rules are
  written again rather than taken out of it. When the last stage lands, `apps/emcraft` runs the
  iced window and `crates/ui-egui` goes, in one change; no fallback binary is kept.
- **Stages.**
  - **A Workflow** (done): the step bar and the kind of analysis, each step's panel (checks and
    fixes, the Setup form over the band's diagram, the device, Run's preflight, the results'
    checks and export) — `emcraft_ui_core::steps`, `crates/ui-iced/src/{steps,step_panel,band}.rs`.
  - **B Results in depth** (done): the S-parameters page (curves, table, a band dragged to solve more
    finely, runs kept and compared, Touchstone), the field's side panel (probes, power), TDR,
    pulse, far-field cuts, dispersion and the study's table, grid and optimisation, chart styles.
  - **C Model input** (done): import (files and dropped files), the templates' dialog with its
    parameters, the browser's eyes, folds and role marks, the dialogs' guidance, the ribbon's
    search, icons (the egui glyphs as shared shapes, `emcraft_render::shapes`), tooltips with the
    command id, the purpose-first start page.
  - **D The window's shell** (done): the status line and the message log, Escape's layers, crash
    reports, recording and replay (the iced window records the commands it was asked to run and
    the pages looked at; an egui recording plays as far as its actions run commands), the
    Japanese fallback font (`emcraft_ui_core::fonts`), pictures written out (the field's Picture…
    and Animation…, landed with B).
  - **E Tests and tools**: the iced scenarios' pointer, key, text and shot steps; the
    translations' coverage over `ui-iced` and `ui-core`; `ui.render` without a GPU (iced's
    tiny-skia renderer).
  - **F Switch**: `apps/emcraft` on iced (`emcraft-iced` folded in), `crates/ui-egui` and egui,
    eframe and glow removed; AGENTS.md, docs/architecture.md, ADRs 0001, 0004, 0007 and
    docs/control-protocol.md brought up to date.
- **Scenarios by task.** An iced scenario is one task a person does, end to end, checked on
  `ui.inspect`'s state (never on pictures, which `shot` only writes for a person to look at).
  Pure rules are tested in `emcraft-ui-core`; what only egui did is dropped. The egui
  scenarios fold into about 35–40:

| iced scenario | egui scenarios it replaces |
|---|---|
| `workflow` (A, done) | workflow_walk, workflow_missing_port, step_status, browser_steps, setup_band, setup_proposal, setup_restore, run_estimate, stale_results, busy_rule |
| `navigation`, `tools` (done) | navigation, view3d_orbit, box_select, view3d_picking, view3d_measure, view3d_port_tool, view3d_lumped_tool, view3d_tools_ja, tool_mode, view3d_side_ports, view3d_ports_before_mesh |
| `view3d_section_symmetry` | section_view, symmetry_eye, symmetry_plane, rotation_sector |
| `purpose_and_kind` | purpose_first, home_analysis_setup, analysis_kind, setup_device |
| `pcb` (two) | pcb_board, pcb_edit, pcb_fem, pcb_fem_model, pcb_region, pcb_route, pcb_templates, pcb_workflow |
| `fields` | field_view, field_probes, field_context, field_plot_zoom, dipole_fields, view3d_field_arrows |
| `import` (C, done) | import_bodies, import_cavity, import_design |
| `hostile_input` | hostile_dialog_input, hostile_step_input (a unit test today), errors_in_place |
| `sparameters` (B, done) | sparameter_display, refine_and_planes, run_history, network_views, chart_style |
| `result_views` (B, done) | tdr_view, pulse_view, far_field_cuts, dispersion_diagram |
| `study` (B, done) | study_grid, study_optimize, study_surrogate, study_sweep |
| `escape_closes_the_top_layer_first` (D, a unit test) | escape_layers |
| dropped (egui only) | chart_packing, small_window, view3d_gpu, wgsl_port_directions |

  The rest (results views, templates, the shell's) are mapped as their stages land.

## Consequences

- Until F, the egui window changes only for a fix; new behaviour goes into `emcraft-ui-core`
  and the iced window.
- The gate keeps both windows' tests until F; then the egui ones go with their crate.
