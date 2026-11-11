# Systems: a chain's budget

A **system** is a chain of two-port stages from a source (an antenna's terminals) to a
receiver, with no field solved: the kind of analysis `system` (`project.analysis`), shown in
the System view as a block diagram, a chart and a budget table. It is how a receiver is checked
once its parts are known — an antenna and a filter solved by EMCraft (or measured), the
amplifiers from their data sheets.

## In the window

A system's job has its own four steps — **Chain** (in the geometry's place), Setup, Run,
Results — and keeps the System view whichever is open: the chain as a block diagram (a click
edits a stage in the Chain step, its menu moves or deletes it; an empty chain offers a button
for each kind), the budget's chart and table. The ribbon's SYSTEM tab adds stages and sets the
source and receiver; the browser lists the source, the stages, the receiver, the measurements
the S-parameter stages read and the budget. No bodies, materials, region or mesh are shown.

## The chain

| Stage (`system.stage.add` `type`) | Settings | Its S | Its noise |
|---|---|---|---|
| `amplifier` | `gain`, `noise_figure` (dB), `oip3`, `p1db` (dBm, output) | matched, unilateral on the chain's impedance | an uncorrelated wave kT₀(F − 1) at its input: Γ_opt = 0, R_n = Z₀(F − 1)/4 |
| `attenuator` | `loss` (dB) | matched | passive at the chain's temperature |
| `data` | `data`: the sweep or a measurement (results.import, `template:` files), `flip` | its S, interpolated | its noise data, else passive (Twiss) |
| `line` | `impedance`, `length`, `eps_r`, `attenuation` (dB/length, ∝ √(f/`at`)), `tan_delta` | cosh/sinh of γl | from its loss |
| `series`, `shunt` | `resistance`, `inductance`, `capacitance`, `arrangement` | an element along or across | its resistance's thermal noise |

`system.stage.edit` changes a stage (the settings given; `rename`, `type`), `.move` and
`.delete` reorder and remove it; every edit is undone by Undo and leaves the last budget stale.
`system.source` sets the antenna (a one-port measurement whose impedance drives the chain:
solve.radiation_sweep's, a .s1p), its noise temperature and the signal it gives;
`system.receiver` the chain's impedance and temperature, the receiver's bandwidth and the SNR
it needs.

## The budget

`solve.system` (Run, the band from the Setup step) reads the chain at each frequency: its S by
the stages' chain matrices, its noise by Hillbrand and Russer's chain correlation matrices from
the source's impedance, and at each stage's output

- the stage's available gain and the chain's from the source (Pozar §12.1),
- the noise figure and temperature so far, the signal (source power + gain), the noise
  k(T_A + T_e)B·G and their ratio,
- the third-order intercept and 1 dB compression points, input-referred as
  1/IIP3 = Σ G_before/IIP3_k (Pozar §10.3; an amplifier's IIP3 = OIP3/G and IP1dB = OP1dB − G + 1 dB) and output-referred.

With a bandwidth the receiver's system temperature T_A + T₀(F − 1) and sensitivity
kT_sysB + SNR follow. Where a data stage or the antenna has no data (outside its band) the
frequency's readings are left out and the reason given. The budget is saved with the project
(docs/project-file.md).

The template `adsb-receiver` is such a chain: the `adsb-patch` antenna's terminals, an LNA, the
`adsb-bandpass` filter and a receiver, the antenna's and the filter's S compiled in as
`template:` Touchstone files solved by EMCraft from those templates; it opens with its budget
read (a system's solve takes a moment, so a sample runs it when it opens).

Checked against closed forms in `crates/engine/src/tests/system.rs`: Friis through an LNA, a
pad and a second stage; gains adding in dB; kT₀B; the cascade's IIP3 and P1dB; a 100 Ω antenna's
mismatch, F_min + (F_min − 1)|Γs|²/(1 − |Γs|²).
