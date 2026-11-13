# ADR 0010: Shapes are made in SolidCraft

Status: accepted (2026-11-13); H1–H6 done.

## Context

The window offers part of a CAD: boxes, cylinders and spheres (`geometry.body`), a waveguide
(`geometry.waveguide`), the region's shape (`geometry.solid`), grips that resize them — but no
sketches, no Booleans the person controls, no history. It is not enough to model with, and it
copies what SolidCraft (the sibling CAD) does properly. The same commands also build 73 of the
79 templates, the oracle's cases and old projects, so they cannot simply go.

## Decision

- **The window makes no shapes.** Geometry comes from a template (its shape set by its
  parameters) or a SolidCraft design (its parameters too, and edited in SolidCraft). Geometry's ＋
  offers the import, a new design in SolidCraft, the templates and new materials (the materials
  are EMCraft's: what a body is made of electromagnetically). A body's form chooses its material;
  its dimensions are the template's or the design's parameters, shown in the tree's Parameters.
  The grips that resized bodies and the waveguide (ADR 0005 P2, ADR 0009 G5) go.
- **The shape commands stay in the engine** — `geometry.solid`, `geometry.body`,
  `geometry.feature`, `geometry.inclusion`, `geometry.waveguide`, `geometry.iris_filter` — as the
  language of scripts, templates, the oracle, agents and saved projects; the window's ＋ lists
  and search leave them out.
- **A round trip with SolidCraft.** An imported design opens in SolidCraft (`solidcraft <file>`;
  where SolidCraft is, in Settings, else on the PATH); saved there, EMCraft reads it again
  (`geometry.reload`: the import and everything after it run again). A port on a design's face
  names the face (`face_name`, the design's persistent face names), so it follows the face
  through an edit; a face gone is an error naming the port, never a port moved silently.
- **Templates test the SolidCraft path.** The templates whose shape is CAD-like (the region's
  shape, bodies, inclusions, features: 41) are rebuilt as SolidCraft designs driven by their
  variables (`geometry.import {parameters: {"a": "=a"}}`); those written in the electromagnetic
  structure itself (the waveguide's sections, layers, strips and vias: 16) and the boards and the
  system (15) stay scripts. A design's source is a SolidCraft command script
  (`<name>.script.json` beside it), built by `solidcraft-cli run … --save <name>.json` (the
  design as JSON, without SolidCraft's evaluated cache) at the commit EMCraft pins and checked
  by `cargo xtask designs --check`; EMCraft still depends on
  `solidcraft-exchange` alone (ADR 0003). Some templates keep a script twin: the two must agree
  with each other and with the closed form, which tests the import, its exact surfaces and the
  mesh. Moving the SolidCraft pin then runs every design template through open, mesh and solve.
- **Size.** A design holds its history, not a mesh: 5–23 kB (1–3 kB deflated); 41 more add under
  1 MB to the repository and the binary.

## Stages

- **H1** (done) The window makes no shapes: the ＋ lists and the search without the shape
  commands, the Parameters in the tree (a template's; a design's with H2), a body's form its
  material, the grips gone.
- **H2** (done) The round trip: a design of the user's is a row in Geometry with ↗ (Open in
  SolidCraft; the program in Settings ▸ SolidCraft, `EMCRAFT_SOLIDCRAFT`, else `solidcraft`), its
  file watched a second at a time and the model read again when it is saved
  (`geometry.reload`); its parameters are rows of the Parameters, set where it is imported
  (`geometry.design`); Geometry's ＋ makes a new design (written empty, opened in SolidCraft,
  imported once saved). A template's design is set by the template's parameters, not opened.
- **H3** (done) Ports by face name: an imported design's bodies keep each triangle's B-rep face
  and each face's persistent name (`Model::faces`, `emcraft_model::faces`);
  `model.port.create {face_name}` spans the named face (flat across x, y or z; the guide's side
  where the model is unless `end` says), found again at every replay; the port tool records a
  named face by its name.
- **H4** (done) Designs from SolidCraft scripts: the templates' seven designs and the oracle's
  post, each with the command script `solidcraft-cli script` wrote from it (`file.new {name}`
  first, so the design keeps its name), their Rust generators gone; `cargo xtask designs` builds
  them (an oracle case's copy of a template's design too) and `--check` compares bytes. Built
  again by the pinned SolidCraft they are the same bytes run to run; against the files the
  generators wrote, only the format's newer spelling moved (`solidcraft/51`, parameter names on
  extrudes, the extent's convention written out), and the templates, the oracle and their
  fixtures did not move.
- **H5** (done) A pilot: six templates moved — the H-plane bend, the cylindrical cavity, the
  coaxial line (its inner conductor a metal body of the design), the inductive post (likewise),
  the metal sphere's scattering (the sphere alone, in the import's open box, its margin `25 − r`
  keeping the box 50 mm) and the cross-guide coupler — each design a short script of
  `solid.box`, `solid.cylinder`, `solid.sphere` and a sketch's `solid.extrude`, the template
  passing its variables (the bend's mitre is `mitre`: `m` is a unit name in SolidCraft). Each
  keeps its old script as its twin (`crates/templates/twins/<name>.json`); `tests/twins.rs`
  solves both and checks they agree with each other and with the closed form where there is
  one (the cylinder's TE111 and TM010, the line's phase, the post's integral equation, the
  sphere's Mie series, the square corner's field superposition at m = 0). A symmetry plane
  across an imported surface now asks each vertex's image to lie on the surface, not to be a
  vertex: a design's two ends are triangulated differently.
- **H6** (done) The other CAD-like templates: 33 more open designs (20 new ones; the
  time-domain twins of a template, the coaxial cavity and the time-domain sphere share theirs),
  so that 39 templates build from SolidCraft designs. Two keep their solids: `dipole`, a box of
  air around a source with no shape to design, and `td-dipole`, whose square arms are drawn on
  the time-domain grid — their ends at cells' centres, where a design's surface (exact to
  rounding) falls on either side and the feed would cross a conductor. The radius bends' arcs are SolidCraft's
  arcs, no longer 36 chords. An open problem keeps its box of air where it was:
  `geometry.import {box}` sets the box's far corner from the design's origin, the bodies left
  where the design puts them. A symmetry plane across a design cut along its extrusion stops it
  there even when its sides are split at heights of their own (every triangle across the plane
  upright, every vertex at or past it on a line through the far end's, none folded over),
  rather than by a Boolean whose cut lies a millionth off the plane. No oracle fixture moved:
  the cases write their own models; the symmetric tee's test meshes at 5 mm (at 6 mm, four
  elements across, the design's whole tee lay 0.015 off its half).
- **H7** Docs and scenarios.
