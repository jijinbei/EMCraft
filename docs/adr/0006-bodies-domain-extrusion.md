# ADR 0006: Bodies, the domain, and the waveguide as an extrusion

Status: proposed (2026-11-07). Steps 1–2 done; steps 3–4 open (below).

## Context

The model kept what it is made of in five places, each with its own way of saying the material:
the waveguide (sections, layers, strips, vias), the region's solid (`solid`, `background`),
the features and inclusions, and an imported part's bodies (`"metal"` / `"interior"`, their
shapes not kept). Its bounds were as scattered: walls, their thickness, the open boundary,
symmetry, periodic and rotational planes. The browser mirrored the commands, not the model.

## Decision

1. **The domain** (`model::domain::Domain`, done): the region's solid and how it is set, its
   background, the outer walls or the open boundary, and the planes that cut it.
2. **The bodies** (`model::body`, done): one list `Model::bodies` in the order placed; each a
   shape, placement, axis, what it is made of (`Fill`: metal, the region's own, a clip, a
   material) and how the region takes it (`Kind`: combined with its shape, meshed apart, an
   imported part's walls or the region itself). The mesher reads the same features and
   inclusions as before through `MeshView` (each list in the bodies' order), so meshes are
   unchanged, bit for bit (the whole oracle). An imported part keeps its surfaces: changing a
   part's material keeps the bodies drawn since.
3. **The waveguide as bodies** (open). `geometry.waveguide` would place a box region and
   bodies; `Model::waveguide` would become a view recognised from them (as `MeshView` is),
   so the waveguide mesher, the time domain and the Floquet ports read it unchanged.
4. **One extrusion mesher** (open): a cross-section mesh (a structured grid, or triangles as
   the PCB's) extruded through slabs, for the waveguide, the PCB and any stack of prisms along
   one axis; the general tetrahedral mesher for the rest; bodies of revolution as now.

## What steps 3–4 need (found on the way)

- **Exact round trip.** The waveguide mesher builds its grid from section lengths and aperture
  spans; a view recognised from bodies must give the same numbers bit for bit, or every
  waveguide oracle moves. A box body's size is exact (the section's length) but its placement
  is a running sum: the view must take lengths from sizes, never from differences of
  placements. Apertures: a section's metal with the aperture put back as a box of the region's
  material keeps the aperture's width and start exact.
- **A sheet shape.** Strips have no thickness: a `Shape::Sheet` (a rectangle in a plane) is
  needed; the general mesher must refuse it until it can mesh one.
- **Sections are slabs, not inclusions.** A section fills the region between two planes; it is
  neither combined with the region nor meshed apart under today's rules (inclusions lie
  strictly inside, apart from each other). A `Kind::Slab` (the extrusion mesher's only) keeps
  that honest.
- **Readers.** 106 reads of `model.waveguide` in 43 files; the writers (geometry.waveguide, the
  templates, the item edits, `project.values`) would write bodies. The view keeps the readers.
- **Ports.** Start and end ports are the region's z sides; `End` can become a side of the
  region's box when the waveguide is a view.

## Consequences

The browser now reads the model (bodies, materials with what uses them, the domain with its
boundary and planes, excitation, mesh, results). Steps 3–4 are their own work, each checked by
the oracle bit for bit.
