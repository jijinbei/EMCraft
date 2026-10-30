# A pyramidal horn from a template's SolidCraft design

The `horn` template is a SolidCraft design compiled into EMCraft
(`crates/templates/designs/horn.solidcraft`, written by `make-horn.rs.txt`): two bodies, the
metal `walls` (2 mm thick round a WR-90 feed 5 mm long and a flare to a 40 × 30 mm mouth 30 mm
further on) and the air `inside` (reaching 1 mm past the mouth, so the surfaces meet in general
position). The template opens it with `geometry.import {path: "template:horn.solidcraft",
materials: {"inside": "vacuum"}, open: true}`, its parameters passed to the design's, in a box
of air lined with a 10 mm perfectly matched layer; the feed's closed end is a waveguide port.

Checked at 10 GHz: the directivity on the axis against Balanis' pyramidal-horn formula and the
power balance. The formula is an approximation (the aperture field only), most accurate for
large horns; for this small one the finite elements give 0.4 dB more (11.54 dBi at four and
11.56 at five elements per wavelength, against 11.16 dBi), and the tolerance, 1 dB, is the
formula's, not the solver's. Not oracle quantities: |S11| = 0.11 at the feed (the mouth's and
the throat's reflections), the E-plane and H-plane patterns.
