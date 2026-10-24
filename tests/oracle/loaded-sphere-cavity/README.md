# Dielectric-loaded spherical cavity

A closed sphere of radius 10 mm with a concentric ceramic sphere (εr = 4) of radius 4 mm. The two
materials are meshed as separate regions — the interface tessellated once and frozen, each
region's tetrahedra built against it, and boundary recovery's added points passed back and forth
until both sides keep the same triangles — and joined node for node. The lowest resonance (the
TM11 triplet) against the two-layer closed form with spherical Bessel functions. The tetrahedra on the
wall and the interface are curved (quadratic geometry); the error is 1e-4.
