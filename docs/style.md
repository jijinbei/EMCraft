# How code is written

One aim: a reader understands a piece of code in one pass, without running it, and can change it
without fear. Every rule below serves that aim; when a rule and the aim disagree, the aim wins.

## 1. Prefer the functional style

- **Expressions over statements.** Build a value with `map`, `filter`, `zip`, `fold`, a `match` or
  an `if` that returns, `Option`/`Result` combinators and `?`, rather than declaring a mutable
  variable and filling it in later.

  ```rust
  // Not this:
  let mut names = Vec::new();
  for p in &ports {
      if p.enabled {
          names.push(p.name.clone());
      }
  }
  // But this:
  let names: Vec<String> = ports.iter().filter(|p| p.enabled).map(|p| p.name.clone()).collect();
  ```
- **Immutable by default.** `let mut` only where building in place is plainly clearer or measurably
  faster: a solver's inner loop, a matrix filled entry by entry.
- **Pure functions at the core, effects at the edges.** A computation takes its inputs and returns
  its result. Reading the session, files or the clock, and writing replies, files or UI state,
  happen in the thin command or view function that calls it. Pure code is testable without
  setup and safe to reuse.
- **Return new values, not out-parameters.** A function that produces three things returns a
  small struct whose fields are named, not a `(f64, f64, Vec<C>)` or three `&mut` arguments.
- **Make wrong states unrepresentable.** An enum instead of a `bool` plus an `Option` that only
  means something when the flag is set. A newtype instead of a bare `f64` when the unit or the
  kind of id matters. A constructor that checks once instead of checks scattered over the uses.
- **Plain beats clever.** A short `for` loop is better than a chain of six closures with nested
  `match`es. If a chain needs a comment to be read, split it into named steps (`let` bindings
  with good names) or write the loop.

## 2. Physics notation

- **Implementing a paper: its notation.** Symbols, signs and normalisations follow the paper the
  code cites, so the code can be read beside it equation by equation.
- **Deriving it yourself: Pozar's.** Where no paper is followed, symbols and conventions are
  those of D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley 2012: time dependence e^{jωt},
  γ = α + jβ, Z₀, Γ, power waves and S, Z, Y and ABCD as its chapter 4 defines them, the
  rectangular and circular guides' modes as chapter 3 does. Cite the section the derivation
  starts from.
- **Say which.** A module's docs name the source of its notation; a departure from it (another
  sign, another normalisation) is stated where it happens.

## 3. Names

- Say what a thing *is*, in words: `reflection_zeros`, `frequencies`, `kept_name`. Physics keeps
  its symbols where the formula is the reference: `omega`, `z0`, `eps_r`, `s21`.
- Put units in the name or the doc: `inductance_nh`, `/// Length (m)`. A number without a unit
  is a bug waiting.
- A function is named for its result (`impedance`, `folded`) or its action (`keep`, `rotate`). A
  `bool` reads as a question (`is_stale`, `fits`). Avoid `data2`, `tmp`, `handle_stuff`, `do_it`.
- One name for one idea across the code: if it is a `measurement` in one module, it is not a
  `dataset` in the next.

## 4. Functions and modules

- **One job each.** If the doc needs "and" twice, split the function.
- **Short and flat.** Aim for a function that fits on a screen, nesting at most three deep. Leave
  early (`?`, `let … else`, a guard `return`) instead of wrapping the body in `if`.
- **Few parameters.** Past four or five, the parameters are a concept: give it a struct.
- **One module per concept,** its public surface small: `pub(crate)` by default, `pub` only for
  what another crate uses.
- **Reuse before you write.** Look for the building block in `docs/architecture.md` first. When the
  same lines appear a second time, factor them out (`cargo xtask dupes` finds copies).

## 5. Comments and docs

- **Docs state the contract:** what the function returns, in which units, for which inputs, and
  how it refuses bad ones. Write them as sentences. A building block others reuse carries a short
  doctest that shows the call.
- **Comments say why, not what.** The code already says what it does. A comment explains a
  choice, a non-obvious invariant, or cites the paper and equation the code follows.
- **No dead text.** Delete commented-out code, stale TODOs and comments that no longer match.

## 6. Numbers and constants

- **No magic numbers.** Caps, tolerances, thresholds and defaults are named constants with a doc
  line saying why that value: `/// The most frequencies of one set (a sweep's cap).`
- **Compare floats with a stated tolerance,** relative where the scale varies, and say where the
  tolerance comes from.

## 7. Errors

- **Return `Result`; never panic** (see AGENTS.md).
- **Errors say what is wrong and how to fix it,** in the user's terms and display units:
  "fewer than 4 frequencies inside the passband: solve more finely, or check the band", not
  "invalid input".
- **Check at the boundary, once.** Validate hostile input where it enters (a command's
  parameters, a file), then pass checked types inward so the core need not check again.

## 8. Tests

- **The test's name is the fact it checks,** as a sentence:
  `a_shunt_resistor_reads_as_its_impedance`.
- **The first line states the evidence:** the closed form, the paper or the symmetry the expected
  value comes from, so a reader can tell the test is right without trusting the code.
- **A test against an exact solution cites it:** the book or paper, with the section, equation
  or table — `// TE10 cut-off f_c = c/2a (Pozar, Microwave Engineering, 4th ed., §3.3).` — the
  same reference `docs/solvers/literature.md` lists. A closed form derived here says so and
  shows the steps a reader needs to check it, from the cited starting point.
- **One behaviour per test,** plus its hostile inputs. Slow tests are marked heavy (AGENTS.md).

## 9. Formatting and text

- **`rustfmt` decides the layout** (`max_width = 150`). No hand alignment, no `#[rustfmt::skip]`.
- **UI text is short:** a word or a short phrase on the screen. Explanations go into Help and
  tooltips, and every shown text is translated (docs/languages.md).
