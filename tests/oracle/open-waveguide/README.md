# Open-ended waveguide

A 20 mm metal block, its top 42 mm up an 80 mm box lined with a 15 mm perfectly matched layer,
holds a WR-90 channel open at the top; its closed bottom (z = 25 mm) is a waveguide port on a
face (`model.port.create` with `face`) whose TE10 feeds the aperture at 10 GHz. Built by
Booleans: the block subtracted from the box, the channel added back (reaching 1 mm above the
block's top, so the surfaces meet in general position).

Checked: the power the port delivers against the power through the Huygens box. Not oracle
quantities (no closed form for a guide opening on a small flange), measured at five elements
per wavelength: |S11| = 0.26 (−11.7 dB), 6.9 dBi on the axis, the E-plane (yz) the broader —
in the range of the classic results for open-ended rectangular guides.
