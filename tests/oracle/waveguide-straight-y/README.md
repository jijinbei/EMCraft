# Empty waveguide along y: ports across y

The `waveguide-straight` case turned onto y: a WR-90 guide (a = 22.86 mm, b = 10.16 mm), 30 mm
long along y, a box solid (`geometry.solid`) whose two faces across y carry the ports
(`model.port.create` with `axis` y and `face`, the rectangle's sides along x and z), solved with
second-order Nédélec elements of about 3 mm at 8, 10 and 12 GHz.

Each port's closed-form modes are those of its face in the port's own axes (u, v, w), turned onto
it; nothing else about a port depends on its axis. The guide is empty, so S11 = 0 and
S21 = exp(−jβL): at 10 GHz β = 158.25 rad/m and S21 = exp(−j·4.7476), a phase of 88.01°, the
same numbers as along z.
