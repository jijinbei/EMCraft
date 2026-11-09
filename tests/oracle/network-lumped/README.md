# Lumped two-ports through their network matrices

Circuits built with `circuit.cascade` (no field solve) and read back by `results.network`,
`results.equivalent` and `results.mixed_mode`, against the textbook closed forms:

- a 100 Ω resistor shunt across the line: every entry of Z is 100 Ω;
- 2 nH in series at 1 GHz: the chain matrix's B is jωL = j12.566 Ω, and the π network fitted to
  it has the inductor in its series branch (a series element has no Z, so no T: refused);
- an L-pad (50 Ω in series, then 50 Ω shunt) on 50 Ω, its two ports taken as one pair: the
  differential reflection Sdd = (S11 − S12 − S21 + S22)/2 = (0.2 − 0.4 − 0.4 − 0.2)/2 = −0.4.
