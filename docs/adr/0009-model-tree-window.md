# ADR 0009: One model tree, a properties panel and the 3D view

Status: accepted (2026-11-13); G1–G4 done.

## Context

The iced window (ADR 0008) shows the same structure three times: the ribbon's tabs (Model,
Materials, Ports & Sources, Boundaries, Mesh, Simulate, Results), the step bar (Geometry,
Excitation, Setup, Region, Mesh, Run, Results) and the browser's sections (Bodies, Materials,
Domain, Excitation, Mesh, Results); the app bar's page tabs are a fourth. A person has to learn
which of them holds what, and dialogs cover the model while they are open.

Alternatives weighed: merging the step bar into the ribbon's tabs (small, but the tree, tabs and
panel stay three), a command palette first (fast for experts, hard to discover), a node graph
and goal-first templates (narrower than the work). Chosen: the model tree is the workflow, what
is selected is edited beside the view, and the 3D view is worked on directly; the search and,
later, an agent are shortcuts over the same commands.

## Decision

- **The tree is the workflow.** The browser's rows grouped under the steps, in order: the
  analysis (its kind, the units), 1 Geometry (bodies, then materials), 2 Excitation, 3 Setup (the
  confirmed settings), 4 Region, 5 Mesh, 6 Run (the run, the studies), 7 Results; a system's
  and a board's own trees likewise. Each step's node carries its status's mark
  (`emcraft_engine::workflow`), ＋ (the commands that add to it) and, for Mesh and Run, ▶. No
  order is enforced; the marks follow the model. The rules are `emcraft_ui_core::outline`.
- **What is selected is edited at the right.** The right column is a properties panel: the
  selected row's item in its edit command's fields (on `project.values`, as its dialog was),
  applied with Enter or Apply, not modal; else the open step's panel (its checks, the Setup form,
  Run's preflight, the results' checks). A command's dialog opened from ＋ or the search docks
  there too. Modal cards stay for files, templates and crash reports.
- **The 3D view is worked on directly.** A face clicked with the port tool is a port, a grip
  dragged changes a length, the panel showing the edited item as it changes.
- **The ribbon, the step bar and the page tabs go.** Every ribbon command is on its step's ＋
  (or the analysis node's, the results'); the command search moves to the app bar; a result's
  node opens its page, the tree staying beside every page of the project.

## Stages

- **G1 Tree as workflow** (done): `emcraft_ui_core::outline` (the steps' nodes over the browser's
  rows); the browser draws it, a step's node opening its panel; the kind of analysis at the
  tree's top; the step bar removed. With it, the start page: its scroll bar at the window's
  edge, no New project button (a blank project starts from the purpose chosen), "not chosen"
  until a kind is.
- **G2 Properties panel** (done): the right column as above; a dialog on a project's page docks
  there, over the window elsewhere; a row selected (in the tree or the 3D view) shows its item's
  form (`Dialog::item`), Apply keeping it open on the new values; a step's node gives the column
  back to the step's panel.
- **G3 ＋ and ▶; the ribbon goes** (done): each step's commands in `emcraft_ui_core::outline`
  (the ribbon's `TABS` folded in; `RibbonTab` kept for old recordings and themes), the search in
  the app bar, the 3D view's bar without the mesh and run buttons and the port tools (Excitation's
  ＋ turns them on).
- **G4 Results in the tree; the page tabs go** (done): a result's row opens its page (the time
  response, the pulse and the analyses under the sweep, the fields on the model), a row of the
  model goes back to the model; `page:<id>` stays as a handle.
- **G5 Direct manipulation:** the port tool from Excitation's ＋, grips on a waveguide's
  sections and box with the panel following.
- **G6 Docs and tests:** docs/ux/workflow.md §2–3, docs/control-protocol.md's handles, the
  scenarios moved over, ROADMAP.md.

Each stage keeps the scenarios and the Japanese tour passing; a stage that removes a handle
moves the scenarios using it in the same change.
