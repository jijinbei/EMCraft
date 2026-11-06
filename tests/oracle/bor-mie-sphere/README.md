# Dielectric sphere scattering a plane wave, as a body of revolution

A sphere of εr = 4 and radius 15 mm at the centre of a vacuum sphere of radius 60 mm lined by
a 20 mm spherical layer, lit at 3 GHz (k a = 0.94) by a plane wave along +z with E along x. The
wave is the orders m = ±1 of the meridian half-plane: m = 1 solved for the field the sphere
scatters (its source k₀²(εr − 1)E_inc in it), m = −1 its mirror. The bistatic radar
cross-section in the E- and H-planes at four angles against the Mie series, evaluated
independently (Bohren and Huffman's coefficients by downward recurrence, in double precision).

The reference script was checked before use: it gives Bohren and Huffman's BHMIE example
(m = 1.55, x = 2π·0.525/0.6328) as published, Q_sca = Q_ext = 3.10543 and Q_back = 2.92534, the
Rayleigh limit 4x⁴|(m² − 1)/(m² + 2)|² at x = 0.01 to 2e-5, the optical theorem (Q_ext = Q_sca
without loss) to twelve digits, and these four angles to twelve digits of
`emcraft_theory::radiation::sphere_rcs`, written apart from it.
