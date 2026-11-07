# Double-plane iris: a 14 × 6 mm window, 2 mm thick, centred in WR-90, at 10 GHz

A guide whose aperture changes in width and height together, meshed by the structured waveguide mesher (cells outside each section's aperture left out, the grid graded towards every edge of one aperture lying inside the other) and solved at order 2. The reference is independent of the finite elements: vector mode matching (`emcraft_theory::modematch::RectLine`), the field on each junction's plane expanded on the two apertures' intersection, its convergence over 300 to 1350 modes stated in `expected.json`.
