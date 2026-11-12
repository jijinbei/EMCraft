# ADR 0005: Geometry as bodies in an automatic region

Status: accepted (2026-11-06). Decision 5 (handles on a body) withdrawn by ADR 0010 (2026-11-13):
the window makes no shapes.

## Context

A model's geometry is today four overlapping ideas, each its own command and word on screen:

| Word | Command | What it is |
|---|---|---|
| Region (領域) | `geometry.solid` | the space solved: one material inside, conducting walls (or a perfectly matched layer) outside |
| Body (物体) | `geometry.body` | something placed in the region by what it is made of: metal (cut out of the region), the region's material (put back), another material (a dielectric) |
| Inclusion (内包物) | `geometry.inclusion` | a dielectric or conducting body inside the region, apart from the others |
| Feature (フィーチャ) | `geometry.feature` | a union, difference or intersection with the region |

`geometry.body` already wraps the last two by material, but all four show. The user also has to
size the region by hand before the ports that end it exist, and to place vacuum as a material
where nothing is. Dimensions are typed as numbers only.

## Decision

1. **Two words: region and body.** The window shows the region and bodies only; a body is metal
   (the default), a dielectric (any other material), or the background put back. Inclusions and
   features stay as the engine's lower-level commands for scripts and saved projects (and what
   `geometry.body` lowers to), not as words on screen.
2. **The background, not vacuum.** Wherever no body is, the model is its background material,
   vacuum by default: nothing is placed for it. One setting changes it (air, a filled device).
   What is not solved — inside metal, outside the region — follows from the bodies and is never
   drawn as a choice. A closed metal shell imported or drawn has the background inside it (no
   cut-then-put-back).
3. **The region is automatic** (overridable). It follows from the bodies, the ports and the band,
   recomputed when any of them changes:
   - **Closed** (metal bodies enclose the rest): the region is the background inside the metal;
     where the enclosure opens, a waveguide port is the region's end there.
   - **Open** (nothing encloses: antennas, scatterers): the bodies' bounding box grown by a margin
     (a quarter of the wavelength at the band's top by default) and lined by the perfectly matched
     layer — `geometry.import`'s `open`/`margin` made general. A body of revolution gets a
     cylinder (or a sphere) about its axis instead of a box.
   - **Ports** cut the region at their planes: along a port's axis the region ends at the port.
   The region is drawn as a translucent box with the layer, and can be pinned (fixed as it is) or
   dragged larger with handles; a pinned region stays until unpinned.
4. **The workflow follows**: Analysis → Geometry (bodies and their materials; the Materials step
   folds into it) → Excitation (ports, sources) → Setup (the band) → Region (automatic: walls,
   layer, symmetry; today's Boundaries) → Mesh → Run → Results. The region is only final once
   the ports and band are, which is why it comes after them.
5. **Handles in the 3D view.** A selected body shows handles: one per face of a box (drag to
   stretch along its normal), radius and length handles on a cylinder or sphere, a move handle at
   its origin and a turn handle about its axis. While dragging, the dimension shows as a number
   at the handle (typed digits replace it), snapping to the grid, to other bodies' faces and
   edges and to the port planes. Each drag ends as one command (`geometry.body.edit` with the new
   dimensions), so undo, history, scripts and the control channel see the same edit; nothing is
   changed outside a command.
6. **Shapes EMCraft draws, and what it leaves to SolidCraft.** EMCraft draws the simple bodies a
   microwave model is made of — box, cylinder, sphere, prism, revolved profile, and the
   waveguide-specific ones (guide sections, irises, posts, tees) — and edits their dimensions.
   Anything more (fillets, sweeps, parametric parts, assemblies) is modelled in SolidCraft and
   imported (ADR 0003), its parameters edited from EMCraft. EMCraft is not a CAD tool.

## Consequences

- New engine commands: `model.background` (the background material), `model.region` (automatic,
  or pinned to a box/cylinder/sphere with its margin and layer), the region's derivation from
  bodies, ports and band (in `emcraft-model`, pure and tested), and a `geometry.body.edit` able
  to change one dimension at a time (for the handles).
- `geometry.solid` stays (scripts, templates, saved projects) and becomes a pinned region.
- The ui-egui scenarios gain handle drags (real pointer drags through the control channel,
  checked on `ui.inspect`), the step bar its new order; the Japanese texts follow.
- Templates keep working: each sets its region today, which reads as pinned.

## Settled

- **A closed device given as its air volume** (the waveguide templates, a filter drawn as its
  cavity): an *interior* body — inside it is solved, everything outside it is metal. It is the
  closed rule's special case (the inside drawn instead of the metal round it), lets the user draw
  the cavity's dimensions without a housing's thickness, and is what `geometry.solid` means:
  today's templates read as one interior body.
- **A dielectric reaching the region's boundary** (a substrate to the box's walls, or into the
  layer as an unbounded one): allowed, clipped by the region. First it is clipped with a warning
  in the reply; the mesher's support for an interface meeting the wall (and a dielectric inside
  the layer, whose tensor takes the material) comes after the automatic region and the handles.
