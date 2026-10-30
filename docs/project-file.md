# Project files (`.emcraft`)

An EMCraft project is one UTF-8 JSON file with the extension `.emcraft`, written by
`file.save` and read by `file.open` (a `.json` file of the same form opens too):

```json
{
  "format": "emcraft-project",
  "version": 3,
  "history": [{"command": "units.set", "params": {"length": "mm"}}, …],
  "runs": [{"command": "solve.frequency", "params": {"start": 8, "stop": 12, "points": 41}, "units": {"length": "mm", "frequency": "GHz"}}],
  "views": {"charts": {"sparameters": {"x": {"min": 6.5e9, "max": 7.5e9}, "hidden": ["s11.ref"]}}, "probes": [{"name": "P1", "at": [0.0114, 0.005, 0.01]}]},
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
  `view.chart.style` and `view.probe.*` (never by the history, so changing them leaves the
  model and its results alone): `charts`, each chart's style by its id (`sparameters`, `tdr`,
  `pattern`, `dispersion`, `study`) — `title`, `x` and `y` (`label`, `min`, `max`, `log`),
  `hidden` (curves' keys), `curves` (a key's `colour` `#rrggbb` and `width` in pixels), `grid`,
  `legend` — with a frequency axis' ends in hertz; and `probes`, each a `name` and a point `at`
  in metres. Only what differs from automatic is written. On reading they are checked like
  everything else (known charts, texts and lists within their caps, finite numbers, valid
  colours, probes named once); otherwise the file is refused. Opening restores them, and a
  change to them leaves the project unsaved.
- `model` is the model as the engine holds it (SI units), for reading by other tools; opening
  does not use it. A port on a face holds `face` as `{"axis", "at", "u0", "v0", "a", "b"}` (its
  plane across x, y or z and its rectangle in the plane's other two axes, in the order x, y, z);
  files written before ports across x and y name them `z`, `x0`, `y0` without an axis, and read
  as a z face.
- Histories written before ports across x and y replay unchanged: `model.port.create`'s `face`
  `[z, x0, y0, a, b]` is the same list with `axis` left out (z).
- `version` is 3 for a project with views, else 2 (which older EMCraft reads). A file without
  one is read as version 1; version 1 files (no `runs`) open as they always did, with no runs.
  A file of a newer version is refused rather than half read.

Saving writes a temporary file beside the target and renames it over, so a crash or a full disk
leaves the old file whole. A path without an extension gets `.emcraft`. The session remembers
the file it was opened from or saved to (`file.save` without a path writes there) and whether
the project changed since (`Session::is_dirty`): its history differs from the saved one, its
views do, or its runs do (a solve with other parameters than the saved run's; one with the same
parameters writes the same file, so it changes nothing). An undo back to the saved state is
clean; the views have no undo of their own.

Undo reaches across projects: undoing `file.open`, `file.new` or `file.sample` brings back the
project before it whole — its file, its sample, its saved state (so the dirty mark is its own),
its runs and the results it had (current again if they were). A save after such an undo writes
that project's own file, never the one opened over it; an untitled project asks for a path.

## Autosave and recovery

`file.save {"path": P, "copy": true}` writes the project to `P` and leaves the session's file,
name and unsaved state as they were: an autosave. (The desktop app's list of recent projects is
its own; a copy is never added to it by the engine.) `file.open {"path": P, "original": O}`
opens such a copy as the project of file `O`: the session takes `O` as its file, and is
unsaved wherever the copy differs from what `O` holds (always, when `O` cannot be read).

The desktop app keeps a list of recent projects in its preferences (`prefs.json` in
`$EMCRAFT_CONFIG_DIR`, `$XDG_CONFIG_HOME/emcraft` or `~/.config/emcraft`) and asks before
unsaved changes would be lost.
