# High-pass filter in the time domain

The `highpass` sample solved by the time-domain solver (`solve.time_domain`): finite
integration on 1 mm hexahedral cells, a Gaussian-modulated TE10 pulse launched in a feed beyond
port 1, PML-terminated feeds at both ends, the TE10 amplitudes on the model's end faces
Fourier-transformed on the fly, and a second run with a uniform guide giving the incident wave.
An independent discretisation of the problem the frequency-domain case `highpass-filter` solves,
against the same closed form.
