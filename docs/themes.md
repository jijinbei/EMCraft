# Themes

A theme sets the colours of the desktop app (its panels, text and accents), of the 3D view and
of the pictures EMCraft draws (charts and field plots, on screen and written by the `view.*`
commands). Colours live only in theme files, JSON in the format below — the built-in themes
too: [`crates/engine/themes`](../crates/engine/themes) `light.json` and `dark.json` name every
colour there is, and the one-hue themes are each a tint of one of them, five numbers
(`cargo xtask colours` keeps colours out of the code). The scientific colour maps of field plots are the same
in every theme: what is drawn over them (arrows, their halo, section lines) is chosen to read on
them.

## The built-in themes

| Name | Look |
|---|---|
| `light` | dark ink on light panels, the default |
| `dark` | light ink on dark panels |
| `mono` | greys only, on light panels |
| `graphite` | greys only, on dark panels |
| `sepia` | warm browns on cream paper |
| `midnight` | deep blues, on dark panels |
| `forest` | greens, on dark panels |

The one-hue themes (`mono` … `forest`) are tints of `light` or `dark` (below): they keep the
lightness of every colour there (so the same contrasts) in their own hue. What has to stay told apart keeps its own hue,
softened: errors, warnings and success, the x, y and z axes, ports, sources and materials in 3D,
and the curves of a chart. The ribbon's tabs share the theme's hue.

## Making one

The quickest: a tint of a built-in theme, in a hue of your own (OKLCH degrees: 30 red, 73 sepia,
150 green, 260 blue) — `{"name": "Teal", "base": "dark", "tint": {"hue": 200, "chroma": 0.06,
"accent": 0.12}}`. Colours named in the file's `ui`, `scene` and `picture` go over the tinted
ones. Or, colour by colour:

1. Copy one of `crates/engine/themes/*.json` (`dark.json`, say): it is the complete template.
2. Give it your `name` and set `"base"` to that built-in theme (`"dark"`): the theme starts from
   that one.
3. Delete every colour you do not change, and change the rest.
4. Check it: `theme.check {"path": "my-theme.json"}` (`emcraft-cli run` a script with that step,
   or `engine.execute` over the control channel) reports the palette pictures get and every
   problem.
5. Put it in the `themes` folder of the configuration directory and choose it in Settings ›
   Appearance (the gear on the app bar, or All settings… on the start page), or with `ui.set {"theme": "<name>"}`.

The configuration directory is `$EMCRAFT_CONFIG_DIR`, else `$XDG_CONFIG_HOME/emcraft`, else
`~/.config/emcraft` (where `prefs.json` lives), so themes go in e.g.
`~/.config/emcraft/themes/my-theme.json`. Every `*.json` file there is read when the app starts
and when the themes are read again (`ui.set {"reload_themes": true}`); at most 32. A file that
cannot be used — not JSON, a bad colour, an unknown name, text that would be hard to read — is
left out and the reason shown once in the messages; nothing else changes.

[`docs/themes/night-shift.json`](themes/night-shift.json) is an example: dark, with warm amber
accents.

```json
{
  "name": "Night shift",
  "description": "Dark, with warm amber accents for late solves.",
  "base": "dark",
  "ui": {"panel": "#1d2129", "text": "#ebe6dc", "accent": "#b5621a"},
  "scene": {"highlight": "#ffb347"},
  "picture": {"ground": "#1d2129", "ink": "#ebe6dc", "series": ["#e0a050", "#6fa8f0"]}
}
```

Pictures written by commands take a theme too: `view.sparameters {"path": "s.png", "theme":
"dark"}` (any built-in theme's name), or the path of a theme file. Without one they are drawn
light.

## The format

A JSON object of at most 64 KiB:

| Key | What |
|---|---|
| `name` | required: 1 to 40 characters, not a built-in theme's name (`light`, `dark`, `mono`, …) |
| `description` | optional, at most 200 characters: shown as the theme's hint in Settings |
| `base` | a built-in theme's name (`light` by default): the theme this one changes. The built-in themes have none, but a tint's (`light` or `dark`) |
| `tint` | optional: every colour of `base` moved in OKLCH by its role (below), before the file's own colours |
| `ui` | the app's colours by name (below), at most 256 |
| `scene` | the 3D view's colours by name (below), opaque |
| `picture` | the pictures' colours by name (below), opaque; `series` a list of up to 6 |

Every colour is `#rrggbb` or `#rrggbbaa` (hex digits of either case; the alpha straight, not
premultiplied). Unknown keys and names are refused with the names there are. A theme is refused
when its text would be hard to read (WCAG contrast): `text` on `panel` and `form_ink` on
`dialog_body` at least 7, `text_dim` on `panel` 3, `on_accent` on `accent` 3, `form_error` on
`form_error_fill` 4.5, each tab's ink on its soft tint and on `toolbar` 4.5; in pictures `ink`
on `ground` 4.5 and `ink_dim` on `ground` 3.

### `tint`

Five numbers, each optional; OKLCH (Ottosson's Oklab in lightness, chroma and hue), where equal
steps look equal:

| Name | What | Default | Range |
|---|---|---|---|
| `hue` | the hue neutral colours and accents take (degrees) | 0 | 0–360 |
| `chroma` | neutral colours' chroma at middle lightness (less towards black and white: `chroma · 4L(1 − L)`) | 0 (greys) | 0–0.37 |
| `lightness` | neutral colours' and accents' lightness scaled by it | 1 | 0.5–1.5 |
| `accent` | accents' chroma at most (the accent, selections, the ribbon's tabs, the kinds of analysis) | 0 (grey) | 0–0.37 |
| `soften` | colours told apart keep their hue and lightness, their chroma scaled by it: errors, warnings and success, the axes, ports, sources, periodic and symmetry planes, materials, the H arrows and a chart's curves | 1 | 0–1 |

Alpha is kept. The tinted theme is checked like any other (the contrasts above).

### `ui`

| Names | What |
|---|---|
| `app_bar`, `app_bar_text`, `app_bar_hover`, `app_bar_active` | the application bar, its text, a hovered button's tint and the open page's (see-through) |
| `toolbar`, `panel`, `panel_header`, `border` | the ribbon's ground, the panels', their headers and edges |
| `text`, `text_dim`, `icon` | text, secondary text, the glyphs |
| `accent`, `accent_soft`, `on_accent` | the accent (selection, links, the active control), its soft tint, ink on it |
| `selected_ink`, `selected_bg`, `hover` | a selected row's ink and ground, a hovered row |
| `error`, `warning`, `success`, `info`, `on_status` | statuses (a step's: missing, warning, done, stale) and the marks on them |
| `form_ink`, `form_dim`, `form_error`, `form_error_fill` | a dialog's text, its labels, a refused field's message and its ground |
| `dialog_body`, `dialog_footer`, `dialog_rule`, `dialog_border` | a dialog card's body, footer, rule and edge |
| `control_fill`, `control_border` | text fields and buttons |
| `viewport_top`, `viewport_bottom` | the 3D view's ground (a gradient) |
| `axis_x`, `axis_y`, `axis_z`, `axis_label` | the 3D view's axes navigator and the letters on it |
| `tab_<tab>_fill`, `tab_<tab>_ink`, `tab_<tab>_soft` | each ribbon tab's accent (`<tab>`: `model`, `materials`, `ports`, `boundaries`, `mesh`, `simulate`, `results`): fill, ink and soft tint |
| `kind_<kind>_ink`, `kind_<kind>_soft` | each kind of analysis's card (`<kind>`: `frequency`, `eigenmodes`, `radiation`, `time_domain`), on the start page and the step bar's switch: its picture's ink and ground |

### `scene`

| Names | What |
|---|---|
| `metal`, `metal_dark` | walls; conductors inside, vias, subtracted features |
| `port`, `port_soft` | ports when they are the subject, and otherwise |
| `port_input`, `port_output` | the driven port (the input: its wave in and the reflection back, S11) and the others (outputs: the wave out, S21 …), in 3D and on the browser's port rows |
| `edge` | the geometry's and the mesh's edges |
| `pml` | the open boundary's layer |
| `source` | dipoles and plane waves |
| `periodic`, `symmetry_electric`, `symmetry_magnetic` | periodic boundaries and symmetry planes |
| `highlight` | what is picked |
| `vacuum`, `material_1` … `material_6` | vacuum, and the other materials by their index (cycled) |
| `mesh_wall` | walls in the mesh overlay |
| `strip`, `intersect` | a layered guide's strips, a feature that intersects the solid |
| `rim` | the rim under the field's arrows |

### `picture`

| Names | What |
|---|---|
| `ground`, `ink`, `ink_dim` | the picture's background, its text and axes, subtitles and reference labels |
| `grid`, `mark` | a chart's grid, its dashed reference lines (cut-offs) |
| `line`, `metal`, `halo` | over a field plot: section boundaries, conductors, the rim round arrows |
| `e_arrows`, `h_arrows` | the electric and magnetic field's arrows |
| `muted` | guides (the light line, a closed form behind the solved points) |
| `series` | the curves' colours in order: blue, red, green, purple, orange, grey in the built-in themes. Solved points over a reference drawn in the same colour are that colour mixed toward `ink`, a reference behind them mixed toward `ground`; a sweep's values run from the first colour to the second |

## Where it lives

- `emcraft_render::palette`: the `Palette` pictures are drawn in and the `Pen` (a colour by role)
  each series and arrow set carries.
- `emcraft_engine::theme`: the file format, its checks and caps, the built-in themes (compiled
  in), `theme.check`, and the `theme` parameter of the `view.*` commands.
- `emcraft_ui_egui::theme`: `Tokens` and `Scene` (their names written once, in a table),
  `Theme`, `Themes` (the built-in and the user's), the current theme of the thread drawing.
