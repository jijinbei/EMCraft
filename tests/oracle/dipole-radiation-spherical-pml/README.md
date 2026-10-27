# Hertzian dipole in a spherical layer

The `dipole-radiation` case with a spherical layer instead of a box's: a 1 mm current element of
1 A at the centre of a 30 mm sphere of vacuum whose outer 15 mm shell (half a wavelength at
10 GHz) is a perfectly matched layer stretched along the radius (F. L. Teixeira, W. C. Chew,
IEEE Microwave Guided Wave Lett. 7(11), 1997), the conducting sphere behind it. The mesh puts
an interface sphere at the layer's inner radius (15 mm), so the layer is a shell of tetrahedra
of its own; the layer's tensors are evaluated at each quadrature point. The power through a
Huygens sphere of 10 mm radius and the directivity from the near-to-far transform of its
equivalent currents, against the closed forms. The dipole sits slightly off the centre so it
lies inside one tetrahedron.

The clearance between the dipole and the layer (15 mm) and the layer's thickness are the box
case's. Unknowns, second-order elements, 10 GHz (the same machine):

| Layer | Elements per wavelength | Unknowns | Power | D(90°) | D(45°) | D(20°) |
|---|---|---|---|---|---|---|
| box, 60 mm (`dipole-radiation`) | 6 | 128 236 | +0.2 % | +1.2 % | +1.1 % | +3.8 % |
| box, 60 mm | 8 | 267 452 | −1.0 % | −0.6 % | −0.4 % | −0.5 % |
| sphere, 30 mm | 6 | 69 352 | +0.7 % | −1.3 % | +4.4 % | +25 % |
| sphere, 30 mm (this case) | 8 | 131 634 | +0.8 % | +1.2 % | −1.4 % | −3.1 % |

At the same element size the sphere holds 46 % fewer unknowns than the box (the box's corners
are layer and padding a compact radiator does not need). At six elements per wavelength both are
limited by the source: the current element is spread over the tetrahedron holding it, and moving
the dipole by a few tenths of a millimetre moves the power by −0.2 to +1.2 % in the box and +0.7
to +2.5 % in the sphere (+5.6 % in a 32 mm one; its mesh puts larger tetrahedra at the centre),
and the directivity near the axis by up to 9 % in the box and 34 % in the sphere; the box
case's placement is a favourable one. The layer itself is not what limits it: its design
reflection from 1e-3 to 1e-9, or a
Huygens box in place of the sphere, change the results by less than the placement does. This case
therefore uses eight elements per wavelength, where the spherical layer meets the box case's
tolerances with about its unknowns — half the box's at the same size (power +0.8 to +2.4 % over
three placements, against −1.0 to −0.6 % for the box at eight per wavelength).
