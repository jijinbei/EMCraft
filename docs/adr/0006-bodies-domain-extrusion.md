# ADR 0006: Bodies, the domain, and the waveguide as an extrusion

Status: accepted (2026-11-07). Steps 1–4 done; what was left out, below.

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
3. **The waveguide as bodies** (done). `geometry.waveguide` places the guide's box (the
   region, `Kind::Extruded`), a slab along z per section (its aperture's box, `Kind::Slab`),
   one across y per layer, a metal sheet per strip (`Shape::Sheet`) and a metal box per via
   (`Kind::Cells`); `Model::waveguide()` is the guide read back from them (cached as
   `MeshView` is), so the waveguide mesher, the time domain and the Floquet ports read it
   unchanged. Lengths come from the boxes' sizes, never from differences of placements; a
   strip's end and a via's top are placed plus size, exact for every guide of the gate. A
   board is one body too (`Shape::Board`, its nets and layers' names its own).
4. **One extrusion mesher** (done): `emcraft_mesh::extrude`, a cross-section (a structured grid
   or a triangulation) extruded through levels along z, each cell at each level filled or left
   out; the waveguide's grid (six Kuhn tetrahedra a cell) and the board's stack (three a prism)
   are both it, each with its own lines, fills and boundaries. `emcraft_mesh::Plan` reads from
   the bodies which mesher a model takes. Every waveguide and board template meshes as before,
   bit for bit, and the whole oracle is unchanged.

## Left out

- **Ports as faces of the region's box.** A port at a guide's start or end and a port on a face
  take different paths to their modes; making the first the second would move results, for
  nothing a user sees (the browser and the legend say either the same way).

## Consequences

The browser reads the model: the bodies (a guide's from its bodies, a board's), the materials
with what uses them, the domain with its boundary and planes, excitation, mesh, results.
