# ADR 0012: Electrons in the fields — tracking first, then particle-in-cell

Status: accepted (2026-11-14); stage A in progress.

## Context

The person wants to send electrons through the structures EMCraft models and watch them; the
goal is a relativistic backward-wave oscillator (BWO), where a beam and the wave of a slow-wave
structure drive each other. That is particle-in-cell (PIC): the fields advanced on a grid, the
particles pushed by them and their currents fed back. An accelerating cavity, whose eigenmodes
and figures (`results.cavity`: V₀, T, R/Q) EMCraft already checks against closed forms, is the
first thing worth seeing and the cheapest to verify.

Two very different computations hide behind "electrons in the fields":

1. **Tracking** — electrons pushed through a field already solved (an eigenmode, a sweep's
   field, a static magnetic field), which they do not change. Its inputs are a result; it adds
   no unknowns.
2. **PIC** — self-consistent: Maxwell's equations in time with the particles' current as a
   source. A run of its own, with a beam, an emitter and static fields as its excitation, and
   particles, phase space and output power as its results.

## Decision

- **Stages.** A: tracking. B: PIC on the time domain's Yee grid. C: beams in cavities (wake
  voltage against the loss factor ω(R/Q)/4, Cherenkov and transition radiation). D: beams in
  slow-wave structures (space-charge depression, limiting current, Pierce's gain, a BWO's start
  current). E: a relativistic BWO against published results. Each stage verified before the
  next (`tests/oracle/`, sources tagged).
- **The pusher in a crate of its own**, `emcraft-particles` (L1, on `emcraft-model` alone): the
  relativistic Boris push (J. P. Boris, *Relativistic plasma simulation — optimization of a
  hybrid code*, Proc. 4th Conf. Num. Sim. Plasmas, 1970; C. K. Birdsall, A. B. Langdon, *Plasma
  Physics via Computer Simulation*, 1991, §15-4) and Vay's (J.-L. Vay, *Simulation of beams or
  plasmas crossing at relativistic velocity*, Phys. Plasmas 15:056701, 2008), which keeps a
  particle in crossed fields that cancel (E = −v×B) on its line at any γ where Boris does not.
  Fields come through a trait, so tracking reads the FEM's solutions and PIC (stage B, in
  `emcraft-fdtd`) the Yee grid's.
- **Tracking is not a kind of analysis.** It is a results command, `results.track`, beside
  `results.cavity`: on an eigenmode (its accelerating voltage V₀ = |∫E_z dz| given, its phase
  set so that φ = 0 is on crest for the particle as it enters) or a sweep's field, with an
  optional uniform static magnetic field. Particles start on the entry plane of a line (the
  region's axis along z by default), feel no field outside the mesh beyond the line's ends, are
  lost where they leave the mesh between them (a wall), and stop past the exit plane. The last
  parameters are kept with the project's views (as `results.pulse`'s are) and the window draws
  the trajectories.
- **PIC is a kind of analysis** (stage B): a row of `workflow::ANALYSES` ("Particles"), its
  excitation the beam and its emitters, its setup the time span, cell and particles per cell,
  its mesh the time domain's grid; charge-conserving deposition (T. Zh. Esirkepov, Comput.
  Phys. Commun. 135:144–153, 2001) so Gauss's law holds without cleaning.
- **Never crash, deterministic.** Steps, particles and energies capped; a non-finite field or
  momentum stops a particle with the reason; particles kept in a fixed order and any random
  emission seeded, so the same run gives the same bits on any number of threads.

## Consequences

Stage A reuses the solved fields as they are and is verified against closed forms (uniform E
and B, E×B drift, an electron in a plane wave — Landau & Lifshitz, *The Classical Theory of
Fields*, §48 — and Vay's crossed fields) and against the pillbox's energy gain eV₀T cos φ. PIC
grows from the time-domain solver without disturbing the frequency domain.
