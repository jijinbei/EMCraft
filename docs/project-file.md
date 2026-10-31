# Project files (`.emcraft`)

An EMCraft project is one UTF-8 JSON file with the extension `.emcraft`, written by
`file.save` and read by `file.open` (a `.json` file of the same form opens too):

```json
{
  "format": "emcraft-project",
  "version": 4,
  "history": [{"command": "units.set", "params": {"length": "mm"}}, …],
  "runs": [{"command": "solve.frequency", "params": {"start": 8, "stop": 12, "points": 41}, "units": {"length": "mm", "frequency": "GHz"}}],
  "views": {"charts": {"sparameters": {"x": {"min": 6.5e9, "max": 7.5e9}, "hidden": ["s11.ref"]}}, "probes": [{"name": "P1", "at": [0.0114, 0.005, 0.01]}]},
  "analysis_history": {
    "next": 2,
    "histories": [[{"command": "units.set", "params": {"length": "mm"}}, …]],
    "solves": [{"id": 1, "when": 1793433600, "command": "solve.frequency", "params": {"start": 6.6, "stop": 9, "points": 3},
                "units": {"length": "mm", "frequency": "GHz"}, "history": 0, "variables": {"l_gap": 20.0}, "pinned": true, "label": "first try",
                "results": {"sweep": {"order": 1, "entries": [{"name": "1", "port": 0}, {"name": "2", "port": 1}], "deembed": [0.0, 0.0],
                                      "frequencies": [6.6e9, 7.8e9, 9e9], "s": [[0.91, -0.2, 0.1, 0.33, 0.1, 0.33, 0.9, -0.21], …]}}}]
  },
  "model": { … }
}
```

- `history` is the script of commands that built the model, in the session's display units at
  the time each ran. Opening a project replays it in a fresh session, so the mesh comes back
  bit for bit (meshing is deterministic). The project's variables are steps of it
  (`project.variable`), and the parameters that use them keep their expressions
  (`"length": "=l"`), so changing a variable, sweeping or optimizing it replays the history with
  the definition's value replaced. Edits and deletions of items (`model.port.edit`,
  `geometry.body.delete`, …) are steps like any other, and so is the kind of analysis chosen at
  the job's start (`project.analysis`): a file without it opens with the kind inferred from the
  model, as before.
- `runs` (since version 2) are the project's runs: the last successful solve of each solve
  command (`solve.frequency`, `solve.eigenmodes`, `solve.time_domain`, `solve.radiation`,
  `solve.dispersion`), the latest last, each with its parameters as given (expressions kept) and
  the display units they were read in. Opening restores them **without running them**:
  `project.runs` lists them (in the display units now) for the Setup step's fields,
  `project.values` gives one command's, and `solve.rerun` runs one again. **Results are not
  stored**: not the S-parameters, fields, eigenmodes or far fields; they are solved again. On
  reading, each run must name a solve command, once, with parameters that command knows (the
  file is not trusted to name any other command); otherwise the file is refused.
- `views` (since version 3) is how the project's results are looked at, set by
  `view.chart.style`, `view.probe.*` and `view.driven` (never by the history, so changing them leaves the
  model and its results alone): `charts`, each chart's style by its id (`sparameters`, `tdr`,
  `pattern`, `dispersion`, `study`) — `title`, `x` and `y` (`label`, `min`, `max`, `log`),
  `hidden` (curves' keys), `curves` (a key's `colour` `#rrggbb` and `width` in pixels), `grid`,
  `legend` — with a frequency axis' ends in hertz; and `probes`, each a `name` and a point `at`
  in metres. Only what differs from automatic is written. On reading they are checked like
  everything else (known charts, texts and lists within their caps, finite numbers, valid
  colours, probes named once); otherwise the file is refused. Opening restores them, and a
  change to them leaves the project unsaved.
- `analysis_history` (since version 4) is the analysis history (`results.history`): the solves
  kept, the oldest first, each with its `id`, `when` (seconds since 1970, UTC), its solve
  `command` and `params` as given in the display `units` they were read in, the `history` that
  built its model (an index into `histories`, where each distinct history of commands is written
  once), its `variables`' values, `pinned` and `label`, and what it found as the history shows
  and compares it (`results`): the sweep's `order`, S's `entries` (each a `name`, its `port`,
  a terminal port's `terminal` and the `reference` impedance in Ω its waves are referred to),
  each port's `deembed` (m), the `frequencies` (Hz) and per frequency S as 2n² numbers, row
  by row, real then imaginary (S[p][q] at 2(pn + q)); the `eigenmodes` (`frequency` in Hz,
  `q_walls`, `q_dielectric`, `q_magnetic`); the time domain's `frequencies`, `s11` and `s21`
  (pairs as S's). **Not the model, the mesh, the fields or the far fields**: the highpass sample
  solved three times over 101 frequencies is a file of 72 kB (4 kB without its history), about
  23 kB a two-port sweep of 101 frequencies, numbers written in full so they read back bit for
  bit (lists of numbers are written on one line). `next` is the id after the last
  given, so ids are never given twice. Opening restores the solves (`results.history` reads as
  it did, number for number, nothing marked current since no results are loaded), and the
  S-parameter chart draws them over each other (`compare`) before anything is solved.
  `results.history.restore` of a solve read back from a file rebuilds its model by replaying
  its history in a fresh session (the mesh comes back bit for bit) and runs its solve again,
  which gives the saved results again (`same_as_saved`), at the cost of its first solve; with
  `solve` false it brings back only the model. On reading, everything is checked: at most 16
  unpinned and 32 pinned solves, ids ascending up to `next`, each `command` a solve command and
  each step of a history a command a history holds (one that changes the model), both with
  parameters they know; at most 2001 frequencies, 64 entries of S and 2²² numbers a solve,
  every size agreeing (2n² numbers an S matrix, one a frequency), every number finite,
  frequencies positive and ascending, impedances positive; labels at most 80 characters.
  Otherwise the file is refused. Files of versions 1 to 3 open with an empty history.
- `model` is the model as the engine holds it (SI units), for reading by other tools; opening
  does not use it. A port on a face holds `face` as `{"axis", "at", "u0", "v0", "a", "b"}` (its
  plane across x, y or z and its rectangle in the plane's other two axes, in the order x, y, z);
  files written before ports across x and y name them `z`, `x0`, `y0` without an axis, and read
  as a z face.
- Histories written before ports across x and y replay unchanged: `model.port.create`'s `face`
  `[z, x0, y0, a, b]` is the same list with `axis` left out (z).
- `version` is 4 for a project with an analysis history, else 3 for one with views, else 2
  (which older EMCraft reads). A file without one is read as version 1; version 1 files (no
  `runs`) open as they always did, with no runs.
  A file of a newer version is refused rather than half read.

Saving writes a temporary file beside the target and renames it over, so a crash or a full disk
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
