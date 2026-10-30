# Languages

The desktop window speaks English (the default) or Japanese, chosen on the settings page
(Settings ▸ Language) or by the control channel (`ui.set {"language": "ja"}`). The window
changes at once and the choice is kept with the preferences (`language` in `prefs.json`).

## How it works

- **Text is looked up by its English**, as gettext does, where it leaves for the screen. The
  window's text helpers take a `lang::Shown`: a `&str` is English to look up, a `String` (or
  `lang::Raw`) is data shown as it is — a name, a number, a path, an engine message.
  - `ui.text(Style::…, "…")` for a line of text, `ui.act(…, hint, …)` and `Response::hint` for
    hover texts, the `ui_kit` controls (`styled`, `button`, `section`, `caption`, `small`, `row`,
    `header`, `error_band`);
  - `lang::tr("…")` and `lang::trf("{n} projects", &[("n", &n)])` where a widget is built by
    hand — placeholders are named, filled after the lookup, so a translation orders them its
    own way;
  - `lang::shown(x)` for a `named_enum!`'s label (views, tabs, steps, statuses, settings
    sections, solvers);
  - commands by id: `crate::label(id)` (`lang::command_label`), `lang::command_doc`,
    `lang::param_doc` — the ribbon, the search, the dialogs and the messages that name a
    command; samples by name (`lang::sample_label`);
  - messages the window writes itself: `app.info_with(template, args)` / `app.error_with`.
- **The translations are data**: `crates/ui-egui/locales/ja.json`, compiled in:
  `{"name": "日本語", "strings": {English: 日本語}, "commands": {id: {label, doc, params:
  {name: doc}}}, "samples": {name: description}}`. A text without a translation shows in
  English.
- **Handles never change**: the control channel's names (`tab:MODEL`, `row:Port 1 (start): 1
  mode`, `settings:language:ja`) are made from the English and the ids whatever is shown, so
  scenarios and agents work in any language. A browser row keeps its English (`Row::text`, its
  handle and key) beside what is shown (`Row::shown`).
- **The font**: egui's own fonts have no Japanese, so a Japanese font is installed after them
  as a fallback (both families, both languages, so Japanese file and project names show too).
  The window looks on a thread at start for one installed on the system
  (`crates/ui-egui/src/fonts/system.rs`): by family — Windows "Yu Gothic UI", "Yu Gothic",
  "Meiryo UI", "Meiryo", "MS UI Gothic", "MS Gothic"; macOS "Hiragino Sans", "Hiragino Kaku
  Gothic ProN"; Linux "Noto Sans CJK JP", "Noto Sans JP", "IPAexGothic", "IPAGothic",
  "TakaoGothic", "VL Gothic" — regular weight first, else any face that shows "設定日本語";
  collections (`.ttc`) by face. The search reads only each file's table directory, names and
  `OS/2`, within caps on folders, files and bytes (about 25 ms for 400 font files). Until it
  ends, and where it finds nothing, the window uses the embedded subset of Noto Sans JP Regular
  (`crates/ui-egui/fonts/`, SIL Open Font License 1.1, under 1 MB): ASCII, Latin-1, general
  punctuation, kana and half-width forms, CJK symbols and full-width forms, JIS X 0208's
  symbols and first-level kanji, and every character of `ja.json` (a test fails otherwise;
  `cargo xtask font-subset` remakes it from the full font in `fonts/full/`). The headless
  harness and `ui.render` use the embedded subset alone, so their pictures are the same on
  every machine; `ui.inspect`'s `font.japanese` names the font in use (an installed family, or
  `embedded subset`).
- **The current language belongs to the thread** drawing the frame (`lang::set`, as the theme
  does), so tests drawing apps on threads of their own keep their own.

## What stays English

- What the engine says: its messages and errors (a refused command, a step's checks), the
  session's default project name.
- The text drawn inside pictures — charts, field plots, the dispersion diagram, the far-field
  pattern: `emcraft-render` sets it in DejaVu Sans, which has no Japanese.
- Ids: command ids, parameter names (the dialogs' field labels, as scripts write them), units,
  symbols (S21, TE10, εr), file names, the theme names of user themes.
- The command line (`emcraft-cli`) and the control channel's replies and errors (bar the
  window's own messages and field errors, which are in the window's language).

## Adding a language or a text

- **A new text in the window**: write it in English through one of the helpers above and add
  its translation to `ja.json`. A text with values is a template with `{name}` placeholders,
  not a `format!`.
- **A new language**: add `crates/ui-egui/locales/<id>.json` in the same format and a line to
  `BUILT_IN` in `crates/ui-egui/src/lang.rs`; the settings page lists it by its `name`. A
  language whose script egui's fonts lack needs a fallback font too (`crates/ui-egui/src/fonts.rs`
  and ATTRIBUTION.md).

## The checks that keep it whole

`cargo test -p emcraft-ui-egui lang::` (`crates/ui-egui/src/lang/tests.rs`):

- every engine command has a Japanese label, description and parameter descriptions, with no
  command or parameter that does not exist; every sample has its description;
- each translation has the same placeholders as its English;
- **a tour of the window in Japanese** draws every page — the start page, every settings
  section, the prompts and menus, ten models with each kind of result (every view, the steps,
  each solver), every command's dialog (some refused), a job running, the search — recording
  every lookup (`lang::record`): it fails on any text shown without a translation, and on any
  translation the tour never asked for (stale), apart from a short list of texts it cannot
  reach, each with why (`UNREACHED`);
- the scenario `crates/ui-egui/tests/scenarios/languages.json` switches to Japanese from the
  settings page, checks that the handles stay English and the messages follow, and switches
  back (`EMCRAFT_SCENARIO_SHOTS=DIR cargo test -p emcraft-ui-egui scenario_languages` writes its
  pictures).
