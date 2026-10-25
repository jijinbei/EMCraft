# Coaxial line

A 10 mm length of PTFE-filled coaxial line (εr = 2.1, conductors of radii 1 and 3.5 mm): a
cylinder with a smaller one subtracted through its whole length, meshed with unstructured
tetrahedra curved onto both conductors. No closed form covers an annular port, so the ports take
their modes from the cross-section solver on their triangles (curved with the volume's) — the
TEM mode first, then the evanescent TE11. A uniform line of one dielectric carries TEM at
k0 √εr exactly, whatever the cross-section, so the transmission's phase and the absence of a
reflection check the numerical port modes and the port condition for hybrid-capable modes.
