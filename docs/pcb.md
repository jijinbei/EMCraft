# PCB route diagnostics

On the start page or the project's Home, choose **PCB signal and return path**. This analysis
uses routed board data and opens the PCB workspace; FEM meshing, modal-port setup
and compute-device selection are not part of this workflow. Imported boards infer the PCB
analysis automatically. `project.analysis {"kind":"pcb"}` declares it in scripts.

The workflow bar above the workspace guides five stages: **Prepare board → Select route →
Set conditions → Run diagnosis → Review results**. Use Next/Back or the available stages in
this bar. First check the top-view layout and layer-stack section, then click a routed net,
set the reference net/impedance/maximum frequency, and press **Inspect route**. Selecting a
net alone does not calculate a result. Missing board/net selections disable later stages.
Stage and net are saved with the project; a saved Results view recomputes its report on reopening.

The layer-stack section shows copper depths, dielectric permittivity and actual thicknesses
beside the 2D layout (drawn spacing is schematic). These dimensions enter the quasi-static
model. Via/pad/bend fields and radiation remain outside this model: a 3D picture alone would
not extend its physics. The supported route screening uses a top view plus stackup.

`view.pcb {"stage":"net"}` navigates; `view.pcb {"stage":"net","net":"DATA"}` selects
without calculation. Stages are `board`, `net`, `settings`, `inspect`, `results`; `pcb.inspect`
performs the diagnosis and advances to Results. All entry points share these engine commands.

Three templates are available from the start page, Home and File → Samples:

| Template | Purpose |
|---|---|
| `pcb-microstrip` | A single outer microstrip over continuous GND; ideal impedance, delay, S and TDR |
| `pcb-split-ground` | The same route over a GND slot; locate missing return reference and withhold S/TDR |
| `pcb-parallel` | A neighbouring CLK trace; demonstrate geometric coupling screening |

Each template has a top-view preview and parameters `l` (routed length), `w` (width), `h`
(substrate thickness), `er` (relative permittivity) and `fmax` (maximum frequency). Defaults
are 40 mm, 0.3 mm, 0.18 mm, 4.2 and 10 GHz. Edit them before opening; subsequent
`project.variable` changes rebuild the PCB and return to board review before another diagnosis. For example:

```json
{"command":"file.sample","params":{"name":"pcb-microstrip","values":{"l":60,"w":0.4,"fmax":5}}}
```

Open **Model → Import PCB**, or drop a `.kicad_pcb` file into EMCraft. Click a routed trace
or a net in the browser, choose its reference net (default `GND`), reference impedance and
maximum frequency, then **Inspect route**. The selected net is highlighted on a top view of
the saved copper. Red spans and circles locate missing reference copper. **Keep for
comparison** pins the current report's finding count while the board is changed or reimported.
The PCB templates demonstrate the difference without an input
file. Saving an `.emcraft` project embeds the imported board; reopening does not need the
original KiCad file. The selected net and last verified conditions are saved; a saved Results stage recomputes its report on reopening.

## What the report means

The geometry checks identify missing filled reference copper below a trace, missing copper
on either side at `h + W/2`, a missing reference plane, unsupported route topology and signal
via transitions. Parallel nearby signal tracks closer than `3h` are flagged for review;
this is a spacing screen, not a crosstalk voltage prediction. A layer change without a nearby
ground via spanning the reference layers is flagged for review (2 mm geometric search).
The via check does not establish electrical connectivity through pads or thermal reliefs.
Trace/polygon intersections are used to locate slots; the check does not sample the trace
at a fixed pitch that could skip a narrow slot. All checks have explicit work limits.

The ideal line estimate uses Hammerstad–Jensen's quasi-static outer microstrip impedance and
effective permittivity, followed by a lossless transmission-line ABCD cascade. It displays
section impedance, propagation delay, S11/S21 magnitudes and a windowed low-pass TDR reflection.
The TDR resolution is limited by the selected maximum frequency and is shown beside the
plot. These are estimates for the stated ideal model, not a 3D FEM result. Bends, pads,
packages, vias, copper thickness, mask, losses, dispersion, coupling and radiation are
excluded. The comparison is a finding count, not a signed-off before/after SI measurement.

S/TDR predictions are withheld for a split/edge crossing, missing reference, branched,
closed, disconnected or self-crossing route, signal via, internal trace, non-adjacent ground,
or width/height outside `0.01..100`. Only a simple single-layer outer trace over a continuous
adjacent reference qualifies. Local section impedance/delay rows remain explicitly labelled
as ideal estimates even when the complete route does not qualify. Copper support alone does
not establish a low-impedance connected return to source/load: isolated islands, pads and
complete current paths need further modelling. No return-current density, eye diagram or
SI/EMC compliance verdict is computed by this feature.

## Import contract

The reader is an original implementation of the public
[KiCad S-expression PCB format](https://dev-docs.kicad.org/en/file-formats/sexpr-pcb/).
It reads straight segments, vias, named nets, explicit dielectric/copper stackup and saved
`filled_polygon` copper. Refill zones in the PCB editor and save before importing. An unfilled
zone outline is skipped with a note; it is never treated as solid ground. Curved tracks are
refused. Pads, footprints, board outlines, mask and copper thickness are excluded with a note.
Coordinates are read in KiCad's millimetres and stored in SI metres.

An absent stackup or missing dielectric permittivity is an error, not an assumed FR-4 board.
Mixed dielectric plies between two copper layers require a caller-supplied homogenized stackup.
`pcb.import` accepts `stackup`, an array of `{name,z,eps_below}` copper layers in increasing
depth order. `z` uses the session's display length unit; `eps_below` is the relative dielectric
permittivity in the interval below that layer (last layer: unused, supply `1`). The file limit
is 8 MiB, with at most 64 copper layers, 20,000 segments, 20,000 vias, 4,096 filled polygons and
200,000 polygon vertices. Inspect a net with at most 512 segments; reports exceeding the
geometric work budget fail visibly. The first release is intended for small routed regions.

## Commands and native board data

All operations use the engine. A CLI script (GHz/mm defaults) can reproduce the example:

```json
[
  {"command":"pcb.sample","params":{"split":false}},
  {"command":"pcb.inspect","params":{"net":"DATA","ground":"GND","reference":50,"stop":1,"points":64}},
  {"command":"file.save","params":{"path":"route.emcraft"}}
]
```

Run it with `cargo run -p emcraft-cli -- run script.json`. Replace the first step with
`{"command":"pcb.import","params":{"path":"board.kicad_pcb"}}` to inspect a real board.
`pcb.inspect` can omit `net` to use the selection or first routed signal; its default maximum
frequency comes from the project's band, or 1 GHz without a band. Replies include per-section reference layers and uncovered intervals (fractions
along each segment), geometry findings, the model assumptions, and either `si` data or
`si: null`. Lengths/frequencies use the current display units; `*_s` and `*_hz` are explicitly
SI. Selection is a view change; importing/loading replaces the current model and can be undone.

`pcb.load` accepts `data` with this native schema (lengths in current display units):

```json
{
  "name":"one route",
  "layers":[{"name":"F.Cu","z":0,"eps_below":4.2},{"name":"B.Cu","z":0.18,"eps_below":1}],
  "segments":[{"id":"track-1","net":"DATA","layer":"F.Cu","start":[5,10],"end":[45,10],"width":0.3}],
  "vias":[],
  "copper":[{"net":"GND","layer":"B.Cu","polygon":[[0,0],[50,0],[50,20],[0,20]]}],
  "notes":[]
}
```

Vias use `{net,at,layers:[from,to],diameter}`. Polygon interiors use the even-odd rule; multiple
polygons on a reference layer are unioned for screening. Native input is checked before any
model replacement; invalid numbers, layers, dimensions and excessive inputs return errors.

Verification: [`pcb-route-line`](../tests/oracle/pcb-route-line/README.md) freezes independently
evaluated HJ/ABCD values for a 40 mm line. Unit tests cover narrow ground slots, topology,
invalid input and ordering; engine tests cover unit conversion, source-independent saving,
reopening and undo; the UI scenario exercises actual route/net clicks and both ground examples.

## Solving a board's region with finite elements

`pcb.region` turns part of the board into the model: a box round the copper (or one net's routes)
with a margin, or between given corners, with air above and below, closed by conducting walls.
The box's lower-left corner is the origin, the lowest layer `below` over the floor; each substrate
becomes a material `substrate εr …`. Ports go on the box's sides with `model.port.create`
(`axis` x or y, `face` the side); their modes come from the cross-section solver, the strips
crossing the side its conductors. `mesh.generate` meshes the region with the planar mesher:

1. Every layer's copper — traces with round ends, vias' pads, filled polygons — in one
   triangulation of the region, on a 1 nm lattice so the same board gives the same mesh;
   overlapping copper of a net is joined, two nets overlapping on a layer are refused by name.
2. The triangles refined to `size` far from copper and to the copper edge size at its edges
   (`strip_edge` × `size`; by default a sixth of the narrowest trace), where the fields are
   singular.
3. The triangulation extruded through the substrates (cells about twice the edge size) and the
   air (cells growing away from the board); the copper conducting sheets of no thickness.

```json
[
  {"command":"pcb.load","params":{"data":{"name":"line","layers":[{"name":"F.Cu","z":0,"eps_below":9.8},{"name":"B.Cu","z":0.635,"eps_below":1}],
    "segments":[{"id":"s","net":"S","layer":"F.Cu","start":[-1,0],"end":[5,0],"width":0.6}]}}},
  {"command":"pcb.region","params":{"lo":[0,-7.2],"hi":[4,7.2],"above":23.4,"below":0}},
  {"command":"model.port.create","params":{"name":"1","end":"start","axis":"x","face":[0,0,0,14.4,24]}},
  {"command":"model.port.create","params":{"name":"2","end":"end","axis":"x","face":[4,0,0,14.4,24]}},
  {"command":"mesh.generate","params":{"size":1}},
  {"command":"solve.frequency","params":{"frequencies":[1],"order":2}}
]
```

Verification: [`pcb-microstrip-fem`](../tests/oracle/pcb-microstrip-fem/README.md) — this line on
alumina against Hammerstad and Jensen: ε_eff within 0.15 %, the 4 mm line's phase within 0.03°.

## Coupling illustration

At **Select route**, choose the source (orange) and receiver (blue), using the role buttons
before clicking a trace or using the receiver list. Swap exchanges the two roles. After
**Inspect route**, Results shows a GPU-rendered 3D schematic linked to the highlighted 2D
parallel overlap. Use its section list to select another overlap; circle marks locate its centre.
Electric, magnetic and return-reference modes explain different relationships. Field strength,
noise voltage, propagation speed and physical phase are **not calculated**. The permanent
schematic banner remains in whole-window PNGs captured by `ui.render`.

The first version only illustrates same-layer straight parallel overlaps with a known reference
layer. Angles, interlayer coupling and vias get no invented field diagram. Ground holes remain
holes; a return-reference line is withheld when the route's reference screening reports gaps
or missing support. Curves and loops are original conceptual drawings, following the mechanisms
explained in Texas Instruments' [Crosstalk on PCB Layouts](https://www.ti.com/content/dam/videos/external-videos/zh-tw/9/3816841626001/6307563213112.mp4/subassets/crosstalk-on-pcb-layouts-presentation-quiz.pdf), not field solutions.

The existing viewport WGSL now generates lit tubular curves from descriptors, shared by the
colour and depth/normal passes; it does not rasterize CPU polylines for these annotations.
Callouts identify the source/receiver nets, their copper layer, the reference net/layer and
the displayed electric/magnetic/return annotation. A translucent block locates the dielectric.
Copper is zero-thickness in the input model; shallow display extrusions distinguish its faces
and are labelled as schematic. Displayed extrusion thickness is not used in the line estimates.

Drag to orbit and scroll to zoom. Depth is exaggerated ×10 by default; toggle it for actual
layer spacing. Local section now opens by default and shortens the traces around the selected
section so that nearby conductors stay readable; turn it off to see the full overlap. Motion defaults
to stopped, and only pulses the emphasis at five updates per second. It is not a wave simulation.
Pair, mode, playback, depth and section choices are saved by `view.pcb` (`victim`, `mode`,
`playing`, `exaggerate`, `section`, `pick_victim`); modes are `electric`, `magnetic`, `return`.
`pcb.inspect.coupling_pairs` replies give both segment ids, source/target endpoints, layer,
clearance and overlap in display units, explicitly tagged `geometry_only`. The geometric
pair search is bounded to one million candidates and 256 output sections; excessive inputs
fail visibly. The illustration also caps drawing work and local ground triangles.

**Keep geometry for comparison** pins a temporary scene in the current workspace. Matching
roles/mode/depth/section settings show before/current pictures with the same camera, plus
clearance and overlap values. No noise-reduction percentage is inferred. New/open project
clears the pin. In `pcb-parallel`, **Edit template spacing…** edits the real `gap` project
variable (default 0.25 mm, positive and at most 5 mm), rebuilding the board. Review it and
select the pair again before diagnosing and comparing. Edited KiCad boards can similarly
be reimported. The ordinary single-line S/TDR estimates below the schematic still exclude coupling.
