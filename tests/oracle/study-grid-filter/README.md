# Parameter sweep on a grid of two section lengths

A five-section filter in WR-90 — dielectric (εr = 2) 15 mm, empty `a`, dielectric `b`, empty
10 mm, dielectric 15 mm — with `a` and `b` variables (`project.variable`, the sections' lengths
given as `"=a"` and `"=b"`). `study.sweep` with `variables` rebuilds the model from the history
at every combination of a = 10, 20 mm and b = 5, 15 mm — geometry, mesh and ports — and solves
each at 6 and 8 GHz; the rows come in the grid's order (b fastest), each keyed by both values.
Every section shares one cross-section, so the cascade of TE10 transmission lines is exact at
each point; the references are that cascade (ABCD matrices of the five lines, Pozar §4.4),
evaluated separately for each (a, b), independently of the solver. Both lengths matter: below
the empty sections' cut-off (6.557 GHz) a longer `a` stops more (|S21| at 6 GHz −3.4 dB at
a = 10 mm, −6.8 dB at 20 mm), and `b` sets where the two evanescent sections' reflections
cancel (|S21| at 8 GHz −3.0 dB at a = 10, b = 15 mm, −1.2 dB at a = 20, b = 15 mm).
