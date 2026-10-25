# Half-wave wire dipole

Two conducting cylinders (radius 0.5 mm, 7 mm long) with a 1 mm gap between them make a
15 mm dipole, half a wavelength at 10 GHz, in a 60 mm box lined with a 15 mm perfectly matched
layer. A lumped port across the gap, its current spread round the wire's surface (`radius`),
drives it. Checked: the power balance between the port and the Huygens box, and the
directivity against the sinusoidal-current closed form.

The input impedance is not an oracle quantity: there is no closed form for a wire this thick.
The thin-wire induced-EMF estimate gives 73 + j43 Ω; the FEM converges to 111 + j27 Ω
(110.9 + j27.9, 110.2 + j27.0, 111.1 + j27.3 Ω at four, five and seven elements per
wavelength), in line with a thin-wire moment-method solution of Hallén's equation for this
radius (about 100 to 114 Ω, itself sensitive to the gap model).
