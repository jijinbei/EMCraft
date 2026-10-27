# Parameter sweep of the high-pass filter

The `highpass-filter` case's model with the length of its empty middle section a variable `l`
(`project.variable`, the section's length given as `"=l"`). `study.sweep` rebuilds the model
from the history at l = 10, 20 and 30 mm — geometry, mesh and ports — and solves each at 6 and
8 GHz. Every section shares one cross-section, so the cascade of TE10 transmission lines is
exact at each length; the references are that cascade (ABCD matrices of the three lines),
evaluated separately for each length, independently of the solver. At l = 20 mm they are the
`highpass-filter` case's own values. Longer evanescent sections stop more below the empty
section's cut-off (6.557 GHz): |S21| at 6 GHz falls from −1.9 to −10.5 dB.
