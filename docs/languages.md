# Languages

The desktop window speaks English (the default) or Japanese, chosen on the settings page
(Settings ▸ Language) or by the control channel (`ui.set {"language": "ja"}`). The window
changes at once and the choice is kept with the preferences (`language` in `prefs.json`).

## How it works

- **Text is looked up by its English**, as gettext does, where it leaves for the screen. The
  window's text helpers take a `lang::Shown`: a `lang::Text` (`t!("…")`) is English to look
  up, a `String` (or `lang::Raw`) is data shown as it is — a name, a number, a path, an engine
  message. The dialogs name parameters in words (`param_labels`) and show choices' values
  (`start`, `auto`, …) in words; the ids stay what scripts write.
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
- **Handles never change**: the control channel's names (`tab:model`, `row:<id>`,
  `command:mesh.generate`) are made from ids whatever is shown, so scenarios and agents work in
  any language.
- **The font**: the window shows Japanese with a font of the system's
  (`crates/ui-core/src/fonts.rs`, `fontdb`: the system's font folders, on
  Linux those fontconfig names) or a font file chosen in Settings ▸ Language (kept with the
  preferences): the chosen file first, then by family — Windows "Yu Gothic UI", "Yu Gothic",
  "Meiryo UI", "Meiryo", "MS UI Gothic", "MS Gothic"; macOS "Hiragino Sans", "Hiragino Kaku
  Gothic ProN"; Linux "Noto Sans CJK JP", "Noto Sans JP", "IPAexGothic", "IPAGothic",
  "TakaoGothic", "VL Gothic" — regular weight upright first, else any face; a face is taken
  when it shows every kana, kanji and full-width character of the translation
  (`lang::script_chars`), 400 faces looked at most (about 0.15 s for 470 faces), and only when
  the window is in a language that needs it. **Nothing is embedded**: where no font shows the
  language, the window stays in English and says once what is missing, what to install
  (Linux: `fonts-noto-cjk`) and that a font file can be chosen; Settings ▸ Language shows the
  font in use or the same error, the file field (Enter) and Look again. `ui.inspect`'s `font`
  names the family in use. The face found also sets the pictures' text (charts and labels drawn on
  the CPU, `emcraft_render::text::set_fallback`); one read from a chosen file is given to iced.
- **The current language belongs to the thread** drawing the frame (`lang::set`, as the theme
  does), so tests drawing apps on threads of their own keep their own.

## What stays English

- What the engine says: its messages and errors (a refused command), the session's default
  project name. Its steps' checks are translated (templates the build checks).
- Text in pictures a chart's templates do not hold (`crate::chart_words::TEMPLATES`: titles,
  axes, legends and marks of the charts are translated, the values they hold in turn;
  `emcraft-render` draws characters DejaVu Sans lacks from the language's font,
  `emcraft_render::text::set_fallback`), and the field plots' and exported PNGs' own words.
- Ids: command ids, units,
  symbols (S21, TE10, εr), file names, the theme names of user themes.
- The command line (`emcraft-cli`) and the control channel's replies and errors (bar the
  window's own messages and field errors, which are in the window's language).

## Adding a language or a text

- **A new text in the window**: write it in English as `t!("…")` and add its translation to
  `ja.json` — the build says where one is missing. A text with values is a template with `{name}` placeholders,
  not a `format!`.
- **A new language**: add `locales/<id>.json` in the same format and a line to
  `BUILT_IN` in `crates/ui-core/src/lang.rs`; the settings page lists it by its `name`. A
  language in another script needs its families in `crates/ui-core/src/fonts.rs`'s `PREFERRED`
  and its characters in `lang::script_chars`.

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

`cargo test -p emcraft-ui-core lang_tests` (`crates/ui-core/src/lang_tests.rs`) and
`cargo test -p emcraft-ui-iced lang_tests` (`crates/ui-iced/src/lang_tests.rs`):

- every engine command has a Japanese label, description and parameter descriptions, with no
  command or parameter that does not exist; every sample has its description;
- each translation has the same placeholders as its English;
- the Japanese keeps its style: spaces between Latin letters or digits and Japanese, no ～, no
  です・ます in a command's texts (`the_japanese_keeps_its_style`);
- **a tour of the window in Japanese** plays every scenario step by step (each step's window
  built, drawn headless on the CPU where it ends), visits every page, step, the message log and
  the settings, the start page with each purpose and every command's dialog, recording every
  lookup (`lang::record`): it fails on any text shown without a translation, and on any
  translation nothing asks for (stale) — not the tour, not a label looked up by name, not a
  `t!` in the sources — apart from a list of texts it cannot reach, each with why
  (`UNREACHED`).
