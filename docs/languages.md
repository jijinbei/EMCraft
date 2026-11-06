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
  - every text written as `t!("…")`, a checked [`lang::Text`]: `lang::tr(t!("…"))` and
    `lang::trf(t!("{n} projects"), &[("n", &n)])` where a widget is built by hand, a `Text`
    wherever a widget takes `impl Shown` (a plain `&str` no longer translates: data is
    `Raw`/`String`) — placeholders are named, filled after the lookup, so a translation orders
    them its own way; a table keeps `t!("…").en()`, its `&'static str` checked the same;
  - `lang::shown(x)` for a `named_enum!`'s label (views, tabs, steps, statuses, settings
    sections, solvers);
  - commands by id: `crate::label(id)` (`lang::command_label`), `lang::command_doc`,
    `lang::param_doc` — the ribbon, the search, the dialogs and the messages that name a
    command; samples by name (`lang::sample_label`);
  - messages the window writes itself: `app.info_with(template, args)` / `app.error_with`.
- **The translations are data**: `locales/ja.json` at the workspace's root, compiled in:
  `{"name": "日本語", "complete": true, "strings": {English: 日本語}, "commands": {id: {label,
  doc, params: {name: doc}}}, "samples": {name: description}}`.
- **Checked when the code compiles** (`emcraft-i18n`): `t!` (the window) and
  `emcraft_i18n::text!` (the engine's step checks) find each text in every complete language
  (`"complete": true`) with the same `{…}` placeholders, or the build stops at the text; a
  text of placeholders and signs only (`{name}: {value}`) needs none. A language still being
  translated is not checked: its missing texts show in English. A test reads every `t!` and
  `text!` in the sources and finds no translation that none of them, the tour of the window or
  a label asks for.
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

- **A new text in the window**: write it in English as `t!("…")` and add its translation to
  `ja.json` — the build says where one is missing. A text with values is a template with `{name}` placeholders,
  not a `format!`.
- **A new language**: add `locales/<id>.json` in the same format and a line to
  `BUILT_IN` in `crates/ui-egui/src/lang.rs`; the settings page lists it by its `name`. A
  language whose script egui's fonts lack needs a fallback font too (`crates/ui-egui/src/fonts.rs`
  and ATTRIBUTION.md).

## How the Japanese is written

One voice across the window, kept by the checks below where a machine can tell.

- **Labels and buttons** are nouns or 「〜を…」: 適用、開く、PCB を読み込む、比較用に保持 — never
  〜する.
- **Tooltips, legends and findings** end in a noun or the plain form, usually without 。:
  「配線の下に参照銅箔がない」「模式図のみ — 電磁界とノイズ電圧は計算していない」.
- **です・ます only where the window speaks to the user**: messages, empty pages, the steps of a
  guide (「PCB を読み込むかサンプルを開き、配線をクリックしてください。」).
- **Command labels, descriptions and parameters** are in the plain form (〜する。〜しない。),
  identifiers and values in backticks: `split`、`true`、`<port>:<k>`.
- **Spacing**: a half-width space between Latin letters or digits and Japanese, and before a
  unit: 3D ビュー、PCB を読み込む、既定 40 mm、90°. The one exception is Sパラメータ (a space
  before it after Japanese: の Sパラメータ). The middle dot ・ joins short nouns without
  spaces; the English 「 · 」 stays as it is.
- **Punctuation**: 「：」 between a label and its value, 「、」 between list items, 「〜」
  (U+301C) for ranges — never ～ (U+FF5E) —, 「」 only to name a control, sparingly.
- **Nothing added, nothing dropped**: a caveat in the English (「計算していない」「規格適合の判定
  ではない」) stays in the Japanese; no instruction the English lacks.
- **One word for one thing**, across the window:

  | English | Japanese |
  |---|---|
  | S-parameters | Sパラメータ |
  | sweep, refine | 掃引、細分化 |
  | excitation, source | 励振、波源 |
  | field | 電磁界 |
  | element order, points | 要素次数、点数 |
  | estimate | 見積もり |
  | zoom | ズーム |
  | default | 既定 |
  | route / trace, net | 配線、ネット |
  | reference copper / plane, ground | 参照銅箔／参照プレーン、GND |
  | return path | 帰路 |
  | inspect, geometry check | 診断、形状チェック |
  | electric / magnetic coupling | 電界結合／磁界結合 |
  | noise source, receiving trace | ノイズ源、受信側 |

- A generic English key (`Source`, `Back`, `Zoom`) keeps its general meaning; a feature that
  needs its own word gets its own key (`Noise source`).

## The checks that keep it whole

`cargo test -p emcraft-ui-egui lang::` (`crates/ui-egui/src/lang/tests.rs`):

- every engine command has a Japanese label, description and parameter descriptions, with no
  command or parameter that does not exist; every sample has its description;
- each translation has the same placeholders as its English;
- the Japanese keeps its style: spaces between Latin letters or digits and Japanese, no ～, no
  です・ます in a command's texts (`the_japanese_keeps_its_style`);
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
