# Half-wave wire dipole

Two conducting cylinders (radius 0.5 mm, 7 mm long) with a 1 mm gap between them make a
15 mm dipole, half a wavelength at 10 GHz, in a 60 mm box lined with a 15 mm perfectly matched
layer. A lumped port across the gap, its current spread round a band at 0.8 of the wire's radius
(`radius` 0.4 mm: just inside the surface, so its lines meet the faceted wire's ends), drives
it. Checked: the power balance between the port and the Huygens box, and the
directivity against the sinusoidal-current closed form.

The input impedance is not an oracle quantity: there is no closed form for a wire this thick.
The thin-wire induced-EMF estimate gives 73 + j43 Ω; the FEM converges to 114 + j36 Ω
(114.7 + j36.3, 114.1 + j36.4, 114.0 + j36.7 Ω at four, five and seven elements per
wavelength), in line with a thin-wire moment-method solution of Hallén's equation for this
radius (about 100 to 114 Ω, itself sensitive to the gap model). The band's radius ρ adds the
inductance μ₀g ln(a/ρ)/2π of the gap g inside the wire's radius a: +j2.8 Ω here, +j25 Ω for a
band of a/5 (115.0 + j53.1 Ω). A band of the wire's radius itself (110.9 + j27.9 Ω) runs
partly outside the faceted wire, beside it, its lines ending in the air.
