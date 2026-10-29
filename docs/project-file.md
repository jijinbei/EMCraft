# Project files (`.emcraft`)

An EMCraft project is one UTF-8 JSON file with the extension `.emcraft`, written by
`file.save` and read by `file.open` (a `.json` file of the same form opens too):

```json
{
  "format": "emcraft-project",
  "version": 1,
  "history": [{"command": "units.set", "params": {"length": "mm"}}, …],
  "model": { … }
}
```

- `history` is the script of commands that built the model, in the session's display units at
  the time each ran. Opening a project replays it in a fresh session, so the mesh comes back
  bit for bit (meshing is deterministic); results are not stored and are solved again. The
  project's variables are steps of it (`project.variable`), and the parameters that use them
  keep their expressions (`"length": "=l"`), so changing a variable, sweeping or optimizing it
  replays the history with the definition's value replaced.
- `model` is the model as the engine holds it (SI units), for reading by other tools; opening
  does not use it.
- `version` is 1. A file without one is read as version 1; a file of a newer version is refused
  rather than half read.

Saving writes a temporary file beside the target and renames it over, so a crash or a full disk
leaves the old file whole. A path without an extension gets `.emcraft`. The session remembers
the file it was opened from or saved to (`file.save` without a path writes there) and whether
its history changed since (`Session::is_dirty`; an undo back to the saved state is clean).

The desktop app keeps a list of recent projects in its preferences (`prefs.json` in
`$EMCRAFT_CONFIG_DIR`, `$XDG_CONFIG_HOME/emcraft` or `~/.config/emcraft`) and asks before
unsaved changes would be lost.
