# Crash reports

EMCraft keeps crash reports on the computer it runs on and sends nothing anywhere, as
SolidCraft does. The code is `crates/ui-core/src/crash.rs` (the window's side in
`crates/ui-egui/src/crash.rs`); what a report replays is `crates/ui-core/src/record.rs`.

## The journal

Every running window keeps a journal in `<config dir>/crashes/journal/`, locked while it runs:
the project's history when it began, then each action asked for (a click, a key, a control
request), with the drafts it reads (the dialog's values at OK, the step panel's fields at a
run). It starts again from the project at each autosave and stops growing at 512 KiB. A clean
close deletes it.

## Reports

- **Crash** (a panic anywhere): the panic hook writes the message, where it happened and the
  backtrace.
- **Closed unexpectedly** (abort, kill, a crash nothing catches): the journal stays, held by no
  one; the next start finds it and makes a report of it.
- **Report** (File ▸ Report a bug…): the journal as it is, for an unexpected result with no
  crash.

A report is a folder in `<config dir>/crashes/`: `report.json` (kind, message, where, the
EMCraft that made it, first and last time, count) and `recording.json`. The same crash again is
counted, not written again. At most 20 reports, 50 MB and 90 days are kept.

Reports not yet seen open on start; File ▸ Crash reports… shows them all. Copy puts the text of
an issue on the clipboard for the person to read and paste; Delete removes the report.

## Playing a report back

`emcraft --replay FOLDER` (or a `recording.json`) opens the window, builds the project from the
recording's history and plays its actions one a frame, each that waits for a job after the one
before. Agents use `ui.replay` and `app.crashes` (docs/control-protocol.md).
