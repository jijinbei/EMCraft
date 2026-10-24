# Waveguide high-pass filter

A WR-90 guide (a = 22.86 mm, b = 10.16 mm) filled with a dielectric of εr = 2 for 15 mm on each
side of a 20 mm empty section. The filled sections carry TE10 from 4.636 GHz; the empty one only
from c/2a = 6.557 GHz. Below that the empty section is cut off: the wave tunnels through it as
an evanescent field decaying as e^{−αz}, α = √((π/a)² − k0²), and the filter stops it
(|S21| = −10.5 dB at 5 GHz); above it the wave passes, with the reflections of the two
dielectric steps (|S21| = −0.42 dB at 9 GHz).

All sections share one cross-section, so TE10 stays TE10 through both steps and the cascade of
transmission lines is exact (the references come from that closed form, computed independently
by ABCD matrices). The fields are checked too: the relative L2 errors of E and H over the whole
guide against the exact TE10 fields. The model is the `highpass` sample (`file.sample`).
