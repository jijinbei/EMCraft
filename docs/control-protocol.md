# Control protocol

`emcraft --control PORT` (or `EMCRAFT_CONTROL_PORT=PORT`) listens on `127.0.0.1:PORT` for JSON
lines, so an agent or a test can drive the running desktop app. Each request is one line
`{"id": …, "method": "…", "params": {…}}`; each reply is one line `{"id": …, "ok": true,
"result": …}` or `{"id": …, "ok": false, "error": "…"}`. Requests arrive as messages of the window
(`crates/ui-iced/src/control.rs`), so each is handled as a click or a key would be; the server,
the token and the request type are `crates/control` (`emcraft-control`). The protocol follows SolidCraft's (`../solidcraft/docs/control-protocol.md`), much reduced.

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
| `ui.inspect` | — | what the window shows: `project` (name, dirty, path), `page`, `settings`, `start`, `tab`, `error`, `messages` (the last, how many, `open`), `job`, `dialog` (its command, fields with text, parts and errors, `options`, `help`, `detected`: what an import's file holds), `asking`, `selection`, `tools` (the tool on, what the pointer snaps to, the points picked, the measurement, the reference plane's drag), `template`, `purpose`, `search` (query, matches), `workflow` (the step open, each step's status and checks, the Setup form, a refusal, a failure), `sparameters`, `view3d` (camera, size, overlay, section, field), `font`, `recording`, `replaying`, `reports`, … |
| `ui.handles` | — | the names `ui.click` presses now: `page:<id>`, `tab:<id>`, `command:<id>`, `overlay:<id>`, `view3d:std:<id>`, `view3d:fit`, `view3d:section`, `tool:<id>`, `toolbar:mesh.generate`, `toolbar:solve.run`, `row:<id>` (its item's form opens in the properties panel), `card:delete`, `step:<id>`, `step:fix:<i>`, `setup:confirm`, `run:start`, `results:check`, `results:export`, `s:display:<id>`, `history:<id>`, `refine:start`, `chart:style`, `style:*`, `purpose:<kind>`, `template:<name>`, `template:build`, `home:import`, `home:blank`, `search:<id>`, `dialog:ok`, `dialog:cancel`, `dialog:pick`, `dialog:options`, `dialog:help`, `ask:save`/`ask:discard`/`ask:cancel`, `reports:close`, `reports:copy:<i>`, `reports:delete:<i>`, `messages:toggle`, `job:cancel`, `file:new`/`file:open`/`file:save`/`file:save_as`, `edit:undo`/`edit:redo`, `start`, `settings` |
| `ui.click` | `handle` | what that handle presses (a file dialog it opens follows on its own) |
| `ui.set` | `page?`, `overlay?`, `tab?`, `settings?`, `start?` (bools), `setup?` (the Setup form's fields by name), `band?` (`[start, stop]`, display units), `pick?` (a frequency clicked on the chart), `template?` (the template card's parameters), `search?` (the command search's query) | the window shown as asked |
| `ui.point` | `at` (`[x, y]` points from the 3D view's corner, within `view3d.size`), `click?`, `double?`, `add?` | the pointer there: what is under it (a tool's face and snap, the field's values), and a click |
| `ui.dialog` | `open?` (a command id), `set?` (`{param: value}`), `parameters?` (an import's design parameters as typed), `ok?`, `cancel?` | a dialog opened, its fields set as typing would, OK (the refusal stays under its field) or Cancel |
| `ui.drop` | `path` | a file dropped on the window: a part (`.solidcraft`, `.sc`, `.3mf`, `.stl`) opens the import dialog in step 1 on it (or goes into the open one), a board (`.kicad_pcb`) the PCB import, anything else is opened as a project |
| `ui.cancel` | — | cancels the running job: the session stays as it was before it |
| `ui.messages` | — | the message area's lines (`text`, `error`, `count`) and the last refusal |
| `ui.record` | `on`, `path?` | `on: true` starts recording what the window is asked to run (commands and their parameters, pages looked at) from the project as it is; `on: false` stops, replying with the recording (written to `path` when given) |
| `ui.replay` | `path` or `recording` | plays a recording back (a file, or a crash report's folder): a new project built from its history, then its entries one a frame, each after the job before it; `ui.inspect`'s `replaying` counts what is left |
| `ui.crashes` | `dir?` | where crash reports go (`dir`: a folder, this window's journal begun there; the desktop app keeps them in `<config dir>/crashes`) and the reports there: id, kind (`panic`, `unclean_exit`, `report`), message, count (docs/crash-reports.md) |
| `ui.render` | `path?`, `size?` (`[w, h]` points, 1–8192; the window's own by default) | the whole window drawn without a window: iced's CPU renderer (tiny-skia), the 3D view drawn offscreen by the GPU into its place (any adapter, a software one included; without any, why); PNG to `path` |
| `ui.screenshot` | `path?` | PNG of the window as presented (needs a visible window; use `ui.render` otherwise) |

## Scenarios

`crates/ui-iced/tests/scenarios/*.json` are user scenarios, each one task end to end, run
headless by `cargo test -p emcraft-ui-iced` (`crate::scenario`): the window built frame by frame
without one and driven by these requests. A step is `{"call": method, "params", "fail"?}`,
`{"click": handle}`, `{"press": text, "double"?}` (the widget showing that text clicked where it
is drawn), `{"field": text or "#id", "text", "enter"?}` (a field clicked and typed over),
`{"key": "Escape" | "Ctrl+Z" | …}` (to the widget that has the keyboard, then the window's own
keys), `{"size": [w, h]}`, `{"frames": n}`, `{"until": checks, "timeout"?}`, `{"expect":
checks}`, `{"shot": name}` or `{"note": …}`; checks are `inspect` (`{path: value}` into
`ui.inspect`, numbers as `[value, rel tol]` or `{"min", "max"}`, `"*"` for present, `{"includes":
text}`), `handles` and `hidden` (`emcraft_ui_core::scenario`). With `EMCRAFT_SCENARIO_SHOTS=DIR`
each `shot` writes the window there as PNG (`ICED_TEST_BACKEND=tiny-skia`: drawn on the CPU).

## Example

```text
{"id":1,"method":"engine.execute","params":{"command":"file.sample","params":{"name":"highpass"}},"token":"…"}
{"id":2,"method":"engine.execute","params":{"command":"solve.frequency","params":{"start":6,"stop":9,"points":7}}}
{"id":3,"method":"ui.click","params":{"handle":"page:sparameters"}}
{"id":4,"method":"ui.render","params":{"path":"/tmp/sparameters.png"}}
```
