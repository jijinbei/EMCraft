# Control protocol

`emcraft --control PORT` (or `EMCRAFT_CONTROL_PORT=PORT`) listens on `127.0.0.1:PORT` for JSON
lines, so an agent or a test can drive the running desktop app. Each request is one line
`{"id": …, "method": "…", "params": {…}}`; each reply is one line `{"id": …, "ok": true,
"result": …}` or `{"id": …, "ok": false, "error": "…"}`. Requests are handled on the UI thread
between frames; pointer and keyboard input is injected as real egui events, one pointer event per
frame. The protocol follows SolidCraft's (`../solidcraft/docs/control-protocol.md`), much reduced.

### Token

Every connection must authenticate. At start the app makes a random 256-bit token for this launch
and writes it, as one line of hex, to a file only the user can read:

| System | Token file |
|---|---|
| Linux | `$XDG_DATA_HOME/emcraft/control/PORT.token` (`~/.local/share/emcraft/control/PORT.token`) |
| macOS | `~/Library/Application Support/EMCraft/control/PORT.token` |
| Windows | `%LOCALAPPDATA%\EMCraft\control\PORT.token` |

On Unix the folder is `0700` and the file `0600`; on Windows the file's inherited permissions are
replaced by one entry for the user. `EMCRAFT_CONTROL_DIR` moves the folder. The file is removed
when the app exits. Its path is printed to stderr at start-up. A test harness may start the app
with `EMCRAFT_CONTROL_TOKEN` set to at least 32 hex characters to use that token instead.

The first request on a connection must carry the token as a top-level `"token"` member; later
requests on the same connection may leave it out. A first line without the right token is
answered with `{"ok": false, "error": "unauthorized: …"}` and the connection is closed without
running anything. Tokens are compared in constant time.

```python
token = (Path.home() / ".local/share/emcraft/control/7878.token").read_text().strip()
sock.sendall((json.dumps({"id": 1, "method": "ui.inspect", "params": {}, "token": token}) + "\n").encode())
```

### Limits

The channel is untrusted input. A request line may be at most 16 MiB (a longer one is answered
with `request too large` and the connection closed); eight lines that are not JSON close the
connection; at most 16 connections are served at once (further ones get `too many
connections`); at most 64 requests are handled per frame; queued input is capped at 10 000
events; coordinates must lie within ±10⁶ points; text is cut to 100 000 characters. A request
waits up to 60 s for the app's reply, an `engine.*` request up to an hour (a long solve).

## Engine

| Method | Params | Result |
|---|---|---|
| `engine.execute` | `command`, `params` (JSON object), `wait?` (default true) | the command's reply. Quick commands run at once; long ones (`solve.*`, `mesh.generate`, `file.sample`, `geometry.import`, `results.compare`, `results.field_error`, `results.far_field`, `project.variable`, `study.sweep`, `study.optimize`) run on the worker thread while the window keeps drawing, and the reply comes when they end. With `wait: false` the reply is `{"started": id}` at once: follow the job with `ui.inspect` (`busy`, `progress`) |
| `engine.script` | `commands`: `[{command, params}, …]` (at most 10 000), `wait?` | the replies, as one job on the worker; stops at the first error and leaves the session as it was |
| `engine.commands` | — | every command: id, label, doc, params, whether it is long |

While a job runs every engine request is refused (`busy: … is running`); `ui.cancel` stops it.
Commands never open dialogs when run this way.

## UI

| Method | Params | Notes |
|---|---|---|
| `ui.inspect` | — | `tab`, `view`, `theme` (the current theme's name), `dark` (whether its panels are dark: `dark`, `graphite`, `midnight`, `forest` and the themes based on them), `themes` (every theme's name: the built-in `light`, `dark`, `mono`, `graphite`, `sepia`, `midnight`, `forest`, then the user's, docs/themes.md), `theme_problems` (the theme files left out and why), `language` (the window's language: `en` or `ja`, docs/languages.md), `languages` (the languages there are), `settings` (the settings page's open section, or null), `dock_dialogs`, the open `dialog` (command, fields as typed, per-field `errors`, `error`, `docked`), `busy`, `progress` (`command`, `done`, `total`, `stage`, `fraction`, `elapsed` s), `status` (the last message: `text`, `error`), `search`, `viewport` rect, `field` (frequency, plane, component, phase, mode, animate, shown), `chart`, `sparameters` (what the S-parameter chart shows: `db`, `magnitude` or `phase`), `variables` (`{name: value}`), `study` (`kind` sweep or optimize, `variables`, `rows`, `converged`, `best` values, `chart`, `parameter`), `port_modes`, `compare` (worst error), `frames`, `model` (`model.summary`), `workflow` (`step`: the open step or null; `steps`: `{step: done|todo|waiting|warning|missing|stale}` (docs/ux/workflow.md §2; `missing` also for the steps a refused Run needed and for Run after a failed run); `running`: a run goes on; `stale`: the results are older than the model; `messages`: each step's checks `{text, command}`; `errors`: the step panels' field errors; `refused`: Run's last refusal; `solver`; `check`: `results.check`'s reply), `view3d` (`yaw`, `pitch` in degrees, `distance`, `target`, `projection`, `overlay` (`plain`, `materials`, `ports`, `boundaries`, `mesh`, `field`), `triangles`, `lines`, `ms` of the last picture, `size`), `undo`, `redo`, `history` (counts), `export` |
| `ui.set` | `tab?` (`MODEL`, `MATERIALS`, `PORTS & SOURCES`, `BOUNDARIES`, `MESH`, `SIMULATE`, `RESULTS`; opening a step selects the tab of its commands), `view?` (`model` (3D), `outline`, `sparameters`, `field`, `port_modes`, `far_field`, `compare`, `eigenmodes`, `study`), `theme?` (a name from `themes`; an unknown one is refused with the names there are), `dark?` (true: the theme `dark`, false: `light`), `reload_themes?` (true: read the user's theme files again first, before `theme`; problems go to the messages and `theme_problems`, a current theme that is gone gives way to `light`), `language?` (`en` or `ja`; an unknown or ill-typed one is refused with the languages there are; handles stay the same in any language), `settings?` (the settings page at a section: `appearance`, `language`, `units`, `3d`, `dialogs`, `files`; `none` closes it), `dock?`, `animate?`, `step?` (`intent` … `results`, or `none`), `standard_view?` (`iso`, `front`, `back`, `top`, `bottom`, `left`, `right`), `camera?` (`{yaw, pitch` in degrees, `distance, projection: perspective|orthographic}`) | returns `ui.inspect` |
| `ui.start` | `command`, `params?` | opens the command's dialog as a ribbon click does, filled with `params` (a parameter it does not take is refused and the dialog closes); commands without parameters run at once |
| `ui.dialog` | `set?` (`{param: value}`), `ok?`, `cancel?` | sets fields as typing would, presses OK (the command's reply, or the refusal; the dialog stays open with the error) or Cancel |
| `ui.cancel` | — | cancels the running job: the session stays as it was before it |
| `ui.messages` | `clear?` | the message area's lines: `text`, `error`, `count` |
| `ui.click` | `x`, `y` (screen points) or `handle`, `button?` (`left`, `right`, `middle`), `double?`, `shift?`, `ctrl?`, `alt?` | real pointer input |
| `ui.move` | `x`, `y` or `handle`, modifiers | pointer movement |
| `ui.drag` | `x0`, `y0`, `x1`, `y1`, `button?`, `steps?` (1–200) | press, move, release |
| `ui.scroll` | `x`, `y`, `delta?` (lines, ±100) | mouse wheel |
| `ui.key` | `key` (egui key name, `Enter`, `Escape`, `Tab`, `A`…), `cmd?`, `shift?` | `{"key": "A", "cmd": true}` selects a field's text |
| `ui.text` | `text` | typed text |
| `ui.handles` | `prefix?` | named points of the frame just drawn, `[{name, x, y}]`: ribbon tabs `tab:<TAB>`, ribbon buttons `toolbar:<command id>` and `toolbar:show:<view>`, view pills `view:<view>`, browser rows `row:<text>` and sections `fold:<section>`, dialog fields `dialog:field:<param>` (vector parts `…:<k>`, table cells `dialog:cell:<param>:<row>:<column>`, `dialog:add:<param>`, `dialog:remove:<param>:<row>`), `dialog:ok`, `dialog:cancel`, `dialog:close`, `dialog:dock`, `search:field`, `search:row:<command id>`, `bar:samples`, `sample:<name>`, `bar:theme`, `bar:dock`, `status:cancel`, the running job's line in the app bar `bar:job` and its Cancel in the right column `job:cancel`, a result view's out-of-date band's `stale:rerun`, `chart:sparameters`, `sparameters:show:<db|magnitude|phase>`, `chart:pattern`, `chart:study`, `study:parameter:<s11|s21|…>`, `picture:field`, `field:plane:<h|e|cross>`, `field:phase`, `field:animate`, `field:frequency`, `pattern:phi`, `eigen:field:<n>`, `port_modes:solve`, `compare:run`; the File and Edit menus `menu:file`, `menu:edit`, `file:save`, `file:open`, `file:export`, `edit:undo`, `edit:redo`, `export:path`, `export:ok`, `export:cancel`; the 3D view `view3d` (its rectangle), `view3d:fit`, `view3d:std:<iso|front|back|top|bottom|left|right>`, `view3d:projection`; the step bar `step:<intent|geometry|…|results>`; the step panel's fixes `step:fix:<i>`, commands `steppanel:<command id>`, `intent:start`, `intent:stop`, `intent:apply`, `excitation:preview`, `excitation:patterns`, `setup:solver:<frequency|eigenmodes|radiation|time_domain>`, `setup:points`, `setup:order`, `setup:count`, `setup:frequency`, `run:start`, `preflight:<blocks|warning>:<i>`, `results:show:<view>`, `results:check`, `results:touchstone`, `results:touchstone:path`, `results:touchstone:<ri|ma>`; the settings page `settings:close`, `settings:section:<appearance|language|units|3d|dialogs|files>`, its choices `settings:<group>:<id>` (`settings:theme:<name>`, `settings:language:<en|ja>`, `settings:length:mm`, …). Handles are the same in every language |
| `ui.at` | `handle` | that handle's point |
| `ui.render` | `path?`, `width?`, `height?` (320–4096), `scale?` (0.5–2) | the whole window drawn on the CPU into an offscreen egui context, no window or GPU needed (`renderer: "cpu"`); PNG to `path` |
| `ui.screenshot` | `path?` | PNG of the window as presented (needs a visible window; use `ui.render` otherwise) |
| `ui.resize` | `width`, `height` | |
| `app.quit` | — | closes the window |

## Scenarios

`crates/ui-egui/tests/scenarios/*.json` are user scenarios run headless by `cargo test -p
emcraft-ui-egui`: the real app drawn frame by frame into an egui context without a window, driven
by these requests (`crate::scenario`). A step is `{"call": method, "params", "fail"?}`, `{"click":
handle or [x, y], "double"?}`, `{"move": …}`, `{"drag": [a, b]}`, `{"widget": label}`, `{"key":
…}`, `{"text": …}`, `{"frames": n}`, `{"until": checks, "timeout"?}`, `{"expect": checks}`,
`{"shot": name}` or `{"note": …}`; checks are `view`, `tab`, `dialog`, `busy`, `status`,
`status_error`, `handles`, `hidden`, `inspect` (`{path: value}` into `ui.inspect`, numbers as
`[value, rel tol]` or `{"min", "max"}`, `"*"` for present), `query` (a quick command's reply)
and `picture`. With `EMCRAFT_SCENARIO_SHOTS=DIR` each `shot` writes the frame there as PNG.

## Example

```text
{"id":1,"method":"engine.execute","params":{"command":"file.sample","params":{"name":"highpass"}},"token":"…"}
{"id":2,"method":"engine.execute","params":{"command":"solve.frequency","params":{"start":6,"stop":9,"points":7}}}
{"id":3,"method":"ui.click","params":{"handle":"view:field"}}
{"id":4,"method":"ui.render","params":{"path":"/tmp/field.png"}}
{"id":5,"method":"app.quit","params":{}}
```
