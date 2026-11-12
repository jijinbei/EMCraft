# ADR 0008: Retiring the egui window

Status: accepted (2026-11-12); done (2026-11-13): stages A–F landed, `apps/emcraft` runs the iced
window and `crates/ui-egui` is gone.

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
  - **E Tests and tools** (done): the iced scenarios' pointer, key, text and shot steps
    (`press`, `field`, `key`, `size`, `shot`, through iced_test's simulator); the translations'
    coverage over `ui-iced` and `ui-core` (the static checks in `emcraft-ui-core`, a tour of the
    iced window in Japanese); `ui.render` without a GPU (iced's tiny-skia renderer, the 3D view
    drawn offscreen into its place).
  - **F Switch** (done): `apps/emcraft` on iced (`emcraft-iced` folded in), `crates/ui-egui` and egui,
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
| `the_section_view_clips_on_the_gpu`, `the_section_shows_its_dimensions_and_looks_at_the_cut`, `the_boundaries_step_finds_the_mirror_planes` (unit tests) | section_view, symmetry_eye, symmetry_plane, rotation_sector |
| `purpose_and_template` (C), `every_steps_panel_and_the_kind` (a unit test) | purpose_first, home_analysis_setup, analysis_kind, setup_device |
| `pcb_diagnosis`, `a_boards_route_is_diagnosed` (a unit test) | pcb_board, pcb_edit, pcb_fem, pcb_fem_model, pcb_region, pcb_route, pcb_templates, pcb_workflow |
| `the_field_overlay_draws_the_plane_with_its_colour_bar`, `the_field_column_reads_power_probes_and_a_pinned_point`, `the_pointer_over_the_plane_reads_the_field` (unit tests) | field_view, field_probes, field_context, field_plot_zoom, dipole_fields, view3d_field_arrows |
| `import` (C, done) | import_bodies, import_cavity, import_design |
| `messages` (D: a refusal in place and in the log), `hostile_setup_input_is_refused_in_place` (a unit test) | hostile_dialog_input, hostile_step_input (a unit test today), errors_in_place |
| `sparameters` (B, done) | sparameter_display, refine_and_planes, run_history, network_views, chart_style |
| `result_views` (B, done) | tdr_view, pulse_view, far_field_cuts, dispersion_diagram |
| `study` (B, done) | study_grid, study_optimize, study_surrogate, study_sweep |
| `escape_closes_the_top_layer_first` (D, a unit test) | escape_layers |
| dropped (egui only) | chart_packing, small_window, view3d_gpu, wgsl_port_directions |

  The rest map to the scenarios of their stages: `purpose_and_template`, `ribbon_search`,
  `import`, `messages`, `keys_and_pointer` and the unit tests beside them.

## Consequences

- The egui window's tests went with its crate; the iced ones, the shared crate's and the
  Japanese tour (`crates/ui-iced/src/lang_tests.rs`) keep what it did.
- What only the egui window had and was not carried over: Export history as script (the
  project file holds the history; `emcraft-cli` runs one), the control channel's raw pointer and
  key requests (`ui.move`, `ui.drag`, `ui.scroll`, `ui.key`, `ui.text`; scenarios press, type
  and key through iced's simulator instead), `ui.start`, `ui.resize`, `app.quit` and
  `app.autosave`. The Japanese strings only it used were taken out of `locales/ja.json`.
