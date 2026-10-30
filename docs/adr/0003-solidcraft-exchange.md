# ADR 0003: SolidCraft designs through `solidcraft-exchange`

Status: accepted (2026-10-31).

## Decision

`geometry.import` opens SolidCraft designs (`.sc`, `.solidcraft`) itself, through one SolidCraft
crate: `solidcraft-exchange`, SolidCraft's typed, stable data view of a design for analysis tools
(its exchange snapshot). EMCraft calls nothing else of SolidCraft: it opens a design, lists its
parameters, sets some of them (`parameters`: name → a number in the design's unit, or an
expression) and takes each body's triangles tagged by B-rep face with each face's exact surface.
The code is `emcraft-io`'s `solidcraft` module (L1); the engine's `geometry.import` reads it as it
reads 3MF, without fitting surfaces the design already knows.

- **Why a crate, not a file or a process.** A design's parameters are evaluated by SolidCraft's
  kernel; going through the crate lets a template's card draw the model again as its parameters
  change, in the same process. 3MF with face tags stays the generic path for any CAD.
- **One facade.** EMCraft depends on `solidcraft-exchange` alone (the user's direction,
  2026-10-28: a big dependency is acceptable, many SolidCraft crates or their internals are
  not). What it pulls in — SolidCraft's geom, kernel, doc, io, sketch, render and derive crates,
  the truck B-rep crates SolidCraft patches, cadmpeg's codecs — comes through it.
- **Pinned.** The dependency is SolidCraft's git repository at one commit (`rev` in the
  workspace's Cargo.toml), and the truck crates are patched from the same commit. A change in
  SolidCraft reaches EMCraft only when that commit is moved, in an EMCraft change whose tests
  and oracle run against it. Until SolidCraft's pull request #8845 (the exchange snapshot) is
  merged, the commit is that pull request's head; afterwards a commit of SolidCraft's main.
- **Toolchain.** `rust-toolchain.toml` pins the stable channel CI uses: a nightly compiler turns
  on `branches`' unstable path (through truck's `rclite`), which later nightlies break.

## Licences

Every crate it adds is MIT, Apache-2.0, or either (SolidCraft: MIT OR Apache-2.0; truck and
cadmpeg: Apache-2.0; `encoding_rs`: (Apache-2.0 OR MIT) AND BSD-3-Clause). None is GPL or LGPL.

## Verified

Oracle `design-post`: a post across WR-90 written as a SolidCraft design with its diameter a
parameter, opened at 4 mm instead of the 3 mm saved, against the closed-form post — |S11| within
3e-5 at 10 GHz by the same adaptive recipe as `imported-post`.
