# Project files (`.emcraft`, version 0.1)

An EMCraft project is one file with the extension `.emcraft`, written by `file.save` and read
by `file.open`: a zip package (`emcraft_io::package`) of `project.json`, readable JSON, and the
arrays of numbers it names, each a part of its own, little-endian, stored as they are:

```
project.json                 the project (below), deflated
mesh/nodes.f64               3 numbers a node (m)
mesh/tets.u32                4 nodes a tetrahedron;  mesh/tet_material.u32  one each
mesh/boundary.u32            6 a boundary face: 3 nodes, its tetrahedron, its condition (kind, value)
mesh/curved.u32, .f64        a curved edge's 2 nodes and its midpoint;  mesh/grid_*.f64, grid_cells.u32
results/sweep/numbers.f64    the sweep's numbers, in order;  results/sweep/fields/<k>.c64  frequency k's fields
results/eigen/…, results/time_domain/…, results/dispersion/…, results/study/…, results/radiation/…, results/td_radiation/…
history/<id>/s.c64           a kept solve's S-parameters (complex: real then imaginary)
```

```json
{
  "format": "emcraft-project",
  "version": "0.1",
  "emcraft": "0.1.0",
  "history": [{"command": "units.set", "params": {"length": "mm"}}, …],
  "runs": [{"command": "solve.frequency", "params": {"start": 8, "stop": 12, "points": 41}, "units": {"length": "mm", "frequency": "GHz"}}],
  "views": {"charts": {"sparameters": {"x": {"min": 6.5e9, "max": 7.5e9}, "hidden": ["s11.ref"]}}, "probes": [{"name": "P1", "at": [0.0114, 0.005, 0.01]}]},
  "analysis_history": {"next": 2, "histories": [[…]], "solves": [{"id": 1, "when": 1793433600, "command": "solve.frequency", …,
                       "results": {"sweep": {"order": 1, "entries": […], "deembed": [0.0, 0.0],
                                             "s": {"frequencies": [6.6e9, 7.8e9, 9e9], "outputs": 2, "inputs": [0, 1], "values": "history/1/s.c64"}}},
                       "provenance": {"version": "0.1.0", "seconds": 1.2, "tetrahedra": 5400, "unknowns": 6800, "order": 1, "linear_solver": "direct", "device": "cpu", "precision": "single", "threads": 8}}]},
  "mesh": {"nodes": 1490, "tetrahedra": 5400, "boundary": 2200, "curved": 0, "grid": [8, 5, 46], "buckets": false},
  "results": {"current": true, "archive_current": 1, "fields": true, "sweep": {"order": 1, "iterative": false, "entries": 2, "points": […], "numbers": "results/sweep/numbers.f64"}},
  "model": { … }
}
```

- `history` is the script of commands that built the model, in the session's display units at
  the time each ran. Opening replays it in a fresh session, every step checked as it runs —
  but the mesh commands (`mesh.generate`, `mesh.adapt`) are read and recorded, not run: the
  model takes the file's own `mesh`. The project's variables are steps of it
  (`project.variable`), and the parameters that use them keep their expressions
  (`"length": "=l"`), so changing a variable, sweeping or optimizing it replays the history —
  then with the mesher — with the definition's value replaced.
- `runs` are the project's runs: the last successful solve of each solve command, the latest
  last, each with its parameters as given (expressions kept) and the display units they were
  read in. `project.runs` lists them for the Setup step's fields and `solve.rerun` runs one
  again. On reading, each run must name a solve command, once, with parameters that command
  knows; otherwise the file is refused.
- `views` is how the project's results are looked at, set by `view.chart.style`,
  `view.probe.*` and `view.driven` (never by the history): each chart's style by its id, and
  the probes. Only what differs from automatic is written; on reading they are checked (known
  charts, texts and lists within their caps, finite numbers, valid colours, probes named once).
- `analysis_history` is the analysis history (`results.history`): the solves kept, the oldest
  first, each with its identity (`id`, `when`, `command`, `params`, `units`, its model's
  `history` — an index into `histories`, each distinct history written once —, `variables`,
  `pinned`, `label`), how it was made (`provenance`) and what it found (`results`: the sweep's
  order, entries, de-embedding and S; the eigenmodes' frequencies and Q; the time domain's S11
  and S21). Restoring one the session holds in memory takes no solve; one read from a file is
  rebuilt from its history and solved again (`same_as_saved` says whether it gave the same).
- `mesh` is the model's mesh: its counts here, its numbers in `mesh/`. On reading it is checked
  (every index in range, every number finite, sizes agreeing and within 2²⁵ tetrahedra, the
  curved edges sorted, the grid ascending; the tetrahedra's shapes by the mesher's check) and
  its point-location buckets built again.
- `results` are the session's results, so a project opens without solving again: the sweep
  (each frequency's S, line impedances, backward error and — when `fields` — its field
  vectors), the eigenmodes (frequencies, Q and vectors), the time domain, the dispersion
  diagram, the study's table (every row, its predictions, the surrogate), the open problem
  (its lumped ports, feed, Huygens samples and field) and the open time domain. Each result's
  shape is here and its numbers a stream in `results/`, read back in the order written. No
  system is written: a sweep's, the eigenmodes' and an open problem's are built again from the
  model and its mesh (an assembly, no factorization) and the vectors read must fit it. A result
  that does not fit is left out (`file.open` says which and why in `left_out`) rather than
  refusing the file (each `{part, why}`). `current` says whether they were the model's results when saved (else
  stale), `archive_current` which kept solve they are. The field vectors are written up to
  1 GB of them (`file.save` `fields`, default true; past the cap, or with `fields` false, the
  S-parameters and tables are written and the fields are solved again).
- `model` is the model as the engine holds it (SI units), for other tools; opening does not
  read it.
- `version` is `"0.1"`. A file of any other version, of none, or not a package (the plain JSON
  of earlier EMCraft) is refused whole rather than half read. Every part read is capped (the
  package 4 GB, `project.json` 64 MB, every array no longer than its shape says).
- The same session saved twice gives the same bytes, but for its solves' wall-clock seconds:
  the package's times are fixed (1980-01-01) and its parts in a fixed order.

Saving writes a temporary package beside the target and renames it over, so a crash or a full disk
leaves the old file whole. A path without an extension gets `.emcraft`. The session remembers
the file it was opened from or saved to (`file.save` without a path writes there) and whether
the project changed since (`Session::is_dirty`): its history differs from the saved one, its
views do, its runs do (a solve with other parameters than the saved run's), or its analysis
history does (a solve kept — every solve that succeeds is, unless it repeats one kept with the
same parameters, model and results, which is that one —, one pinned, labelled or let go).
Saving itself changes nothing in the project. An undo back to the saved state is clean; the
views have no undo of their own.

Undo reaches across projects: undoing `file.open`, `file.new` or `file.sample` brings back the
project before it whole — its file, its sample, its saved state (so the dirty mark is its own),
its runs, its analysis history and the results it had (current again if they were). A save after such an undo writes
that project's own file, never the one opened over it; an untitled project asks for a path.

## Autosave and recovery

`file.save {"path": P, "copy": true}` writes the project to `P` (its analysis history too) and leaves the session's file,
name and unsaved state as they were: an autosave. (The desktop app's list of recent projects is
its own; a copy is never added to it by the engine.) `file.open {"path": P, "original": O}`
opens such a copy as the project of file `O`: the session takes `O` as its file, and is
unsaved wherever the copy differs from what `O` holds (always, when `O` cannot be read).

The desktop app keeps a list of recent projects in its preferences (`prefs.json` in
`$EMCRAFT_CONFIG_DIR`, `$XDG_CONFIG_HOME/emcraft` or `~/.config/emcraft`) and asks before
unsaved changes would be lost.
